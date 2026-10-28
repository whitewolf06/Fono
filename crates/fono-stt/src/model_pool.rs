//! Bounded, multi-model ownership for a server runtime. Desktop keeps its
//! single `SttEngine`; every pooled slot owns an independent engine.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use fono_core::OperationCancellation;
use fono_stt_protocol::{DecodingOptions, WindowTranscript};
use parking_lot::Mutex;

use crate::{
    error::{SttError, SttResult},
    types::{AccelerationMode, ModelId, SttModelSelection, Transcript},
    SttEngine, SttEnvironment, SttHealth, SttReadiness,
};

/// `Resident` suppresses timer-based unloading. Both policies remain eligible
/// for inactive LRU eviction when the configured hard capacity is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelResidency {
    Resident,
    IdleUnload(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelConfiguration {
    selection: SttModelSelection,
    acceleration: AccelerationMode,
    residency: ModelResidency,
    estimated_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Loading,
    Loaded,
    Retiring,
}

struct Slot {
    configuration: ModelConfiguration,
    engine: Arc<SttEngine>,
    state: SlotState,
    detached: bool,
    borrowers: usize,
    last_used: Instant,
    lru: u64,
}

#[derive(Default)]
struct PoolState {
    slots: HashMap<ModelId, Slot>,
    // Persist successful identities across eviction. In particular, every
    // custom Whisper path has variant `custom` and must not alias another file.
    known: HashMap<ModelId, ModelConfiguration>,
    tick: u64,
    closed: bool,
}

impl PoolState {
    fn touch(&mut self, id: &ModelId) {
        self.tick = self.tick.saturating_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.lru = self.tick;
            slot.last_used = Instant::now();
        }
    }

    fn estimated_bytes(&self) -> u64 {
        self.slots.values().fold(0u64, |total, slot| {
            total.saturating_add(slot.configuration.estimated_bytes)
        })
    }
}

trait Lifecycle: Send + Sync {
    fn quiescent(&self, engine: &SttEngine) -> bool;
    fn load(
        &self,
        engine: &SttEngine,
        selection: &SttModelSelection,
        acceleration: AccelerationMode,
        cancellation: &OperationCancellation,
    ) -> SttResult<()>;
    fn retire(&self, engine: &SttEngine) -> bool;
    /// This must be a nonblocking observation. A detached worker still occupies
    /// its pool reservation until its supervised cleanup has truly finished.
    fn cleanup_pending(&self, engine: &SttEngine) -> bool;
}

struct EngineLifecycle;

impl Lifecycle for EngineLifecycle {
    fn quiescent(&self, engine: &SttEngine) -> bool {
        engine.is_quiescent_for_pool()
    }

    fn load(
        &self,
        engine: &SttEngine,
        selection: &SttModelSelection,
        acceleration: AccelerationMode,
        cancellation: &OperationCancellation,
    ) -> SttResult<()> {
        engine.ensure_model_loaded(selection, acceleration, cancellation)
    }

    fn retire(&self, engine: &SttEngine) -> bool {
        engine.retire_idle_for_pool()
    }

    fn cleanup_pending(&self, engine: &SttEngine) -> bool {
        engine.cleanup.is_pending()
    }
}

struct Shared {
    environment: SttEnvironment,
    max_loaded_models: usize,
    max_estimated_bytes: Option<u64>,
    state: Mutex<PoolState>,
    // Serialize only new loads and retirements. Existing loaded models can be
    // acquired while another model is loading.
    admission: Mutex<()>,
    lifecycle: Arc<dyn Lifecycle>,
}

/// Snapshot of occupied reservations. `retiring` counts detached workers whose
/// cleanup still owns resources; it is never treated as free capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPoolStatus {
    pub loaded: usize,
    pub loading: usize,
    pub retiring: usize,
    /// Caller supplied admission estimates, not measured RAM or VRAM.
    pub estimated_bytes: u64,
}

#[derive(Clone)]
pub struct ModelPool {
    shared: Arc<Shared>,
}

impl ModelPool {
    pub fn new(environment: SttEnvironment, max_loaded_models: usize) -> SttResult<Self> {
        Self::build(
            environment,
            max_loaded_models,
            None,
            Arc::new(EngineLifecycle),
        )
    }

    /// Optional admission cap based on estimates supplied by the caller for
    /// each selection. This is not a physical memory measurement.
    pub fn with_estimated_memory_limit(
        environment: SttEnvironment,
        max_loaded_models: usize,
        max_estimated_bytes: u64,
    ) -> SttResult<Self> {
        Self::build(
            environment,
            max_loaded_models,
            Some(max_estimated_bytes),
            Arc::new(EngineLifecycle),
        )
    }

    fn build(
        environment: SttEnvironment,
        max_loaded_models: usize,
        max_estimated_bytes: Option<u64>,
        lifecycle: Arc<dyn Lifecycle>,
    ) -> SttResult<Self> {
        if max_loaded_models == 0 || max_estimated_bytes == Some(0) {
            return Err(SttError::Config(
                "Model pool limits must be positive".into(),
            ));
        }
        Ok(Self {
            shared: Arc::new(Shared {
                environment,
                max_loaded_models,
                max_estimated_bytes,
                state: Mutex::new(PoolState::default()),
                admission: Mutex::new(()),
                lifecycle,
            }),
        })
    }

    /// Acquire a pinned model. `estimated_bytes` is an admission estimate; it
    /// must be positive if a memory cap was configured. A model identity is
    /// permanently bound to one path, acceleration, policy and estimate for
    /// the lifetime of this pool, including after eviction.
    pub fn acquire(
        &self,
        selection: &SttModelSelection,
        acceleration: AccelerationMode,
        residency: ModelResidency,
        estimated_bytes: u64,
        cancellation: &OperationCancellation,
    ) -> SttResult<ModelLease> {
        super::validate_model_selection(selection)?;
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        if self.shared.state.lock().closed {
            return Err(shutting_down());
        }
        if self.shared.max_estimated_bytes.is_some() && estimated_bytes == 0 {
            return Err(SttError::Config(
                "A positive model memory estimate is required by this pool".into(),
            ));
        }
        if self
            .shared
            .max_estimated_bytes
            .is_some_and(|limit| estimated_bytes > limit)
        {
            return Err(SttError::Config(
                "Model memory estimate exceeds the pool limit".into(),
            ));
        }
        let configuration = ModelConfiguration {
            selection: selection.clone(),
            acceleration,
            residency,
            estimated_bytes,
        };

        loop {
            match self.try_existing(&configuration)? {
                Existing::Ready(lease) => {
                    return if cancellation.is_cancelled() {
                        Err(cancelled())
                    } else {
                        Ok(lease)
                    };
                }
                Existing::Idle | Existing::Retiring | Existing::Loading | Existing::Missing => {}
            }

            // A pending caller may cancel while another model spends seconds
            // loading. Do not hold the pool state mutex across that wait.
            let admission = loop {
                if cancellation.is_cancelled() {
                    return Err(cancelled());
                }
                if let Some(guard) = self
                    .shared
                    .admission
                    .try_lock_for(Duration::from_millis(25))
                {
                    break guard;
                }
            };
            self.reap_retired();
            match self.try_existing(&configuration)? {
                Existing::Ready(lease) => {
                    drop(admission);
                    return if cancellation.is_cancelled() {
                        Err(cancelled())
                    } else {
                        Ok(lease)
                    };
                }
                Existing::Idle => return self.refresh_idle(&configuration, cancellation),
                Existing::Retiring => return Err(capacity_busy()),
                Existing::Loading => continue,
                Existing::Missing => {}
            }

            self.make_room(estimated_bytes, cancellation)?;
            if cancellation.is_cancelled() {
                return Err(cancelled());
            }

            let mut environment = self.shared.environment.clone();
            // A shared observer cannot identify which model changed state.
            // The pool exposes per-model readiness through its lease instead.
            environment.readiness = None;
            let engine = Arc::new(SttEngine::new(environment));
            {
                let mut state = self.shared.state.lock();
                if state.closed {
                    return Err(shutting_down());
                }
                state.slots.insert(
                    selection.id.clone(),
                    Slot {
                        configuration: configuration.clone(),
                        engine: Arc::clone(&engine),
                        state: SlotState::Loading,
                        detached: false,
                        borrowers: 1,
                        last_used: Instant::now(),
                        lru: 0,
                    },
                );
                state.touch(&selection.id);
            }

            let result = if self.shared.state.lock().closed {
                Err(shutting_down())
            } else {
                self.shared
                    .lifecycle
                    .load(&engine, selection, acceleration, cancellation)
            };
            let result = if self.shared.state.lock().closed {
                Err(shutting_down())
            } else {
                result
            };
            match result {
                Ok(()) if !cancellation.is_cancelled() => {
                    let mut state = self.shared.state.lock();
                    let slot = state
                        .slots
                        .get_mut(&selection.id)
                        .expect("loading reservation remains owned by admission gate");
                    slot.state = SlotState::Loaded;
                    state.known.insert(selection.id.clone(), configuration);
                    drop(state);
                    return Ok(ModelLease {
                        shared: Arc::clone(&self.shared),
                        id: selection.id.clone(),
                        engine,
                    });
                }
                result => {
                    // A failed worker startup may have a process still under
                    // supervised cleanup. Keep the reservation until both
                    // detachment and cleanup have completed.
                    self.shared
                        .state
                        .lock()
                        .slots
                        .get_mut(&selection.id)
                        .expect("loading reservation")
                        .state = SlotState::Retiring;
                    if self.shared.lifecycle.retire(&engine) {
                        self.mark_detached(&selection.id, &engine);
                        if !self.shared.lifecycle.cleanup_pending(&engine) {
                            self.remove_retired(&selection.id, &engine);
                        }
                    }
                    return Err(result.err().unwrap_or_else(cancelled));
                }
            }
        }
    }

    /// An idle slot may have lost its worker while no borrower was present.
    /// Refresh only under admission, after fencing new borrowers, and never
    /// while a cancelled request still owns native/worker resources.
    fn refresh_idle(
        &self,
        configuration: &ModelConfiguration,
        cancellation: &OperationCancellation,
    ) -> SttResult<ModelLease> {
        let id = &configuration.selection.id;
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        let engine = {
            let mut state = self.shared.state.lock();
            if state.closed {
                return Err(shutting_down());
            }
            let slot = state.slots.get_mut(id).expect("idle slot");
            debug_assert_eq!(slot.state, SlotState::Loaded);
            debug_assert_eq!(slot.borrowers, 0);
            slot.state = SlotState::Loading;
            slot.borrowers = 1;
            Arc::clone(&slot.engine)
        };
        if !self.shared.lifecycle.quiescent(&engine) || cancellation.is_cancelled() {
            self.restore_idle(id, &engine);
            return Err(if cancellation.is_cancelled() {
                cancelled()
            } else {
                capacity_busy()
            });
        }
        let result = if self.shared.state.lock().closed {
            Err(shutting_down())
        } else {
            self.shared.lifecycle.load(
                &engine,
                &configuration.selection,
                configuration.acceleration,
                cancellation,
            )
        };
        let result = if self.shared.state.lock().closed {
            Err(shutting_down())
        } else {
            result
        };
        match result {
            Ok(()) if !cancellation.is_cancelled() => {
                self.shared
                    .state
                    .lock()
                    .slots
                    .get_mut(id)
                    .expect("refresh slot")
                    .state = SlotState::Loaded;
                Ok(ModelLease {
                    shared: Arc::clone(&self.shared),
                    id: id.clone(),
                    engine,
                })
            }
            result => {
                let mut state = self.shared.state.lock();
                let slot = state.slots.get_mut(id).expect("refresh slot");
                slot.state = SlotState::Retiring;
                slot.borrowers = 0;
                drop(state);
                if self.shared.lifecycle.retire(&engine) {
                    self.mark_detached(id, &engine);
                    if !self.shared.lifecycle.cleanup_pending(&engine) {
                        self.remove_retired(id, &engine);
                    }
                }
                Err(result.err().unwrap_or_else(cancelled))
            }
        }
    }

    fn restore_idle(&self, id: &ModelId, engine: &Arc<SttEngine>) {
        let mut state = self.shared.state.lock();
        let slot = state.slots.get_mut(id).expect("refresh slot");
        debug_assert!(Arc::ptr_eq(&slot.engine, engine));
        slot.state = SlotState::Loaded;
        slot.borrowers = 0;
    }

    fn try_existing(&self, configuration: &ModelConfiguration) -> SttResult<Existing> {
        let mut state = self.shared.state.lock();
        if state.closed {
            return Err(shutting_down());
        }
        if state
            .known
            .get(&configuration.selection.id)
            .is_some_and(|known| known != configuration)
        {
            return Err(identity_conflict());
        }
        let Some(slot) = state.slots.get(&configuration.selection.id) else {
            return Ok(Existing::Missing);
        };
        if slot.configuration != *configuration {
            return Err(identity_conflict());
        }
        match slot.state {
            SlotState::Loading => Ok(Existing::Loading),
            SlotState::Retiring => Ok(Existing::Retiring),
            SlotState::Loaded => {
                if slot.borrowers == 0 {
                    return Ok(Existing::Idle);
                }
                let engine = Arc::clone(&slot.engine);
                let slot = state
                    .slots
                    .get_mut(&configuration.selection.id)
                    .expect("existing slot");
                slot.borrowers += 1;
                state.touch(&configuration.selection.id);
                Ok(Existing::Ready(ModelLease {
                    shared: Arc::clone(&self.shared),
                    id: configuration.selection.id.clone(),
                    engine,
                }))
            }
        }
    }

    fn make_room(
        &self,
        estimated_bytes: u64,
        cancellation: &OperationCancellation,
    ) -> SttResult<()> {
        let mut attempted = HashSet::new();
        loop {
            if cancellation.is_cancelled() {
                return Err(cancelled());
            }
            self.reap_retired();
            let candidate = {
                let mut state = self.shared.state.lock();
                if state.closed {
                    return Err(shutting_down());
                }
                let fits_count = state.slots.len() < self.shared.max_loaded_models;
                let fits_memory = self.shared.max_estimated_bytes.is_none_or(|limit| {
                    state
                        .estimated_bytes()
                        .checked_add(estimated_bytes)
                        .is_some_and(|total| total <= limit)
                });
                if fits_count && fits_memory {
                    return Ok(());
                }
                // Retiring workers and borrowed models cannot be freed by
                // this admission. Avoid evicting other idle models when their
                // removal still could not make the new model fit.
                let pinned: Vec<_> = state
                    .slots
                    .values()
                    .filter(|slot| slot.state != SlotState::Loaded || slot.borrowers != 0)
                    .collect();
                if pinned.len() >= self.shared.max_loaded_models
                    || self.shared.max_estimated_bytes.is_some_and(|limit| {
                        pinned.iter().fold(estimated_bytes, |sum, slot| {
                            sum.saturating_add(slot.configuration.estimated_bytes)
                        }) > limit
                    })
                {
                    return Err(capacity_busy());
                }
                let victim = state
                    .slots
                    .iter()
                    .filter(|(id, slot)| {
                        slot.state == SlotState::Loaded
                            && slot.borrowers == 0
                            && !attempted.contains(*id)
                    })
                    .min_by_key(|(_, slot)| slot.lru)
                    .map(|(id, slot)| (id.clone(), Arc::clone(&slot.engine)));
                if let Some((id, _)) = &victim {
                    let slot = state.slots.get_mut(id).expect("victim exists");
                    slot.state = SlotState::Retiring;
                    slot.detached = false;
                }
                victim
            };
            let Some((id, engine)) = candidate else {
                return Err(capacity_busy());
            };
            if self.shared.lifecycle.retire(&engine) {
                self.mark_detached(&id, &engine);
                if !self.shared.lifecycle.cleanup_pending(&engine) {
                    self.remove_retired(&id, &engine);
                }
            } else {
                let mut state = self.shared.state.lock();
                if let Some(slot) = state.slots.get_mut(&id) {
                    slot.state = SlotState::Loaded;
                }
                attempted.insert(id);
            }
        }
    }

    fn remove_retired(&self, id: &ModelId, engine: &Arc<SttEngine>) {
        let removed = {
            let mut state = self.shared.state.lock();
            if state.slots.get(id).is_some_and(|slot| {
                slot.state == SlotState::Retiring
                    && slot.detached
                    && Arc::ptr_eq(&slot.engine, engine)
            }) {
                state.slots.remove(id)
            } else {
                None
            }
        };
        drop(removed);
    }

    fn mark_detached(&self, id: &ModelId, engine: &Arc<SttEngine>) {
        let mut state = self.shared.state.lock();
        if let Some(slot) = state.slots.get_mut(id) {
            if slot.state == SlotState::Retiring && Arc::ptr_eq(&slot.engine, engine) {
                slot.detached = true;
            }
        }
    }

    fn reap_retired(&self) {
        let retiring: Vec<_> = {
            let state = self.shared.state.lock();
            state
                .slots
                .iter()
                .filter(|(_, slot)| slot.state == SlotState::Retiring)
                .map(|(id, slot)| (id.clone(), Arc::clone(&slot.engine), slot.detached))
                .collect()
        };
        for (id, engine, detached) in retiring {
            if !detached {
                if !self.shared.lifecycle.retire(&engine) {
                    continue;
                }
                self.mark_detached(&id, &engine);
            }
            if !self.shared.lifecycle.cleanup_pending(&engine) {
                self.remove_retired(&id, &engine);
            }
        }
    }

    /// Reap expired `IdleUnload` slots on demand. Hosts can call this from a
    /// maintenance tick; there is no hidden timer or background thread.
    pub fn reap_idle(&self) -> usize {
        let Some(_admission) = self.shared.admission.try_lock() else {
            return 0;
        };
        self.reap_retired();
        let expired: Vec<_> = {
            let mut state = self.shared.state.lock();
            let now = Instant::now();
            let entries: Vec<_> = state
                .slots
                .iter()
                .filter(|(_, slot)| {
                    slot.state == SlotState::Loaded
                        && slot.borrowers == 0
                        && matches!(slot.configuration.residency, ModelResidency::IdleUnload(after) if now.duration_since(slot.last_used) >= after)
                })
                .map(|(id, slot)| (id.clone(), Arc::clone(&slot.engine)))
                .collect();
            for (id, _) in &entries {
                let slot = state.slots.get_mut(id).expect("expired slot");
                slot.state = SlotState::Retiring;
                slot.detached = false;
            }
            entries
        };
        let mut retired = 0;
        for (id, engine) in expired {
            if self.shared.lifecycle.retire(&engine) {
                retired += 1;
                self.mark_detached(&id, &engine);
                if !self.shared.lifecycle.cleanup_pending(&engine) {
                    self.remove_retired(&id, &engine);
                }
            } else if let Some(slot) = self.shared.state.lock().slots.get_mut(&id) {
                slot.state = SlotState::Loaded;
            }
        }
        retired
    }

    /// Prevent new model acquisitions. Existing leases remain valid so a host
    /// can first drain or cancel accepted requests before retiring workers.
    /// Calls waiting on the admission gate observe this fence on their next
    /// attempt; a load already in progress is retired when it returns.
    pub fn begin_shutdown(&self) {
        self.shared.state.lock().closed = true;
    }

    /// Retire every idle model, including `Resident`, and reap completed
    /// worker cleanup. This is safe to call repeatedly after `begin_shutdown`.
    /// A borrowed or natively busy model remains reserved until it can be
    /// retired; the returned status must not be mistaken for completion.
    pub fn reap_shutdown(&self) -> ModelPoolStatus {
        if !self.shared.state.lock().closed {
            return self.status();
        }
        let Some(_admission) = self.shared.admission.try_lock() else {
            return self.status();
        };
        self.reap_retired();
        let idle: Vec<_> = {
            let mut state = self.shared.state.lock();
            let entries: Vec<_> = state
                .slots
                .iter()
                .filter(|(_, slot)| slot.state == SlotState::Loaded && slot.borrowers == 0)
                .map(|(id, slot)| (id.clone(), Arc::clone(&slot.engine)))
                .collect();
            for (id, _) in &entries {
                let slot = state.slots.get_mut(id).expect("idle shutdown slot");
                slot.state = SlotState::Retiring;
                slot.detached = false;
            }
            entries
        };
        for (id, engine) in idle {
            if self.shared.lifecycle.retire(&engine) {
                self.mark_detached(&id, &engine);
                if !self.shared.lifecycle.cleanup_pending(&engine) {
                    self.remove_retired(&id, &engine);
                }
            }
        }
        self.status()
    }

    /// True only after the pool was closed and every worker reservation and
    /// supervised cleanup has been released.
    pub fn shutdown_complete(&self) -> bool {
        let state = self.shared.state.lock();
        state.closed && state.slots.is_empty()
    }

    pub fn status(&self) -> ModelPoolStatus {
        let state = self.shared.state.lock();
        let mut status = ModelPoolStatus {
            loaded: 0,
            loading: 0,
            retiring: 0,
            estimated_bytes: state.estimated_bytes(),
        };
        for slot in state.slots.values() {
            match slot.state {
                SlotState::Loaded => status.loaded += 1,
                SlotState::Loading => status.loading += 1,
                SlotState::Retiring => status.retiring += 1,
            }
        }
        status
    }

    pub fn loaded_model_ids(&self) -> Vec<ModelId> {
        let state = self.shared.state.lock();
        let mut models: Vec<_> = state
            .slots
            .iter()
            .filter(|(_, slot)| slot.state == SlotState::Loaded)
            .map(|(id, _)| id.clone())
            .collect();
        models.sort_by_key(ModelId::public_id);
        models
    }

    /// Read one model without borrowing it or triggering a load. A retiring
    /// reservation has no published readiness to report.
    pub fn model_readiness(&self, id: &ModelId) -> Option<SttReadiness> {
        let engine = {
            let state = self.shared.state.lock();
            state
                .slots
                .get(id)
                .filter(|slot| slot.state == SlotState::Loaded)
                .map(|slot| Arc::clone(&slot.engine))
        }?;
        Some(engine.readiness())
    }

    /// Health may probe a worker, so it runs after releasing the pool lock.
    pub fn model_health(&self, id: &ModelId) -> Option<SttHealth> {
        let engine = {
            let state = self.shared.state.lock();
            state
                .slots
                .get(id)
                .filter(|slot| slot.state == SlotState::Loaded)
                .map(|slot| Arc::clone(&slot.engine))
        }?;
        Some(engine.health())
    }
}

enum Existing {
    Ready(ModelLease),
    Idle,
    Loading,
    Retiring,
    Missing,
}

/// A request owns this guard until it has finished using the recognizer. The
/// guard cannot be cloned or converted to an untracked `Arc<SttEngine>`.
pub struct ModelLease {
    shared: Arc<Shared>,
    id: ModelId,
    engine: Arc<SttEngine>,
}

impl ModelLease {
    pub fn model_id(&self) -> &ModelId {
        &self.id
    }

    pub fn readiness(&self) -> SttReadiness {
        self.engine.readiness()
    }

    pub fn health(&self) -> SttHealth {
        self.engine.health()
    }

    pub fn device(&self) -> String {
        self.engine.device()
    }

    pub fn transcribe_segments(
        &self,
        samples: &[i16],
        segments: &[fono_audio::speech_segments::SpeechSegment],
        language: &str,
        cancellation: &OperationCancellation,
        decoding: DecodingOptions,
    ) -> SttResult<WindowTranscript> {
        self.engine
            .transcribe_segments(samples, segments, language, cancellation, decoding)
    }

    pub fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> SttResult<Transcript> {
        self.engine
            .transcribe_cancellable(samples, language, cancellation)
    }

    pub fn transcribe_cancellable_with_options(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        decoding: DecodingOptions,
    ) -> SttResult<Transcript> {
        self.engine
            .transcribe_cancellable_with_options(samples, language, cancellation, decoding)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transcribe_window_with_options(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
        decoding: DecodingOptions,
    ) -> SttResult<WindowTranscript> {
        self.engine.transcribe_window_with_options(
            samples,
            language,
            context,
            operation_id,
            cancellation,
            audio_start_sample,
            decoding,
        )
    }
}

impl Drop for ModelLease {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock();
        if let Some(slot) = state.slots.get_mut(&self.id) {
            debug_assert!(Arc::ptr_eq(&slot.engine, &self.engine));
            debug_assert!(slot.borrowers > 0);
            slot.borrowers -= 1;
            state.touch(&self.id);
        }
    }
}

fn cancelled() -> SttError {
    SttError::Cancelled("Model pool request cancelled".into())
}

fn shutting_down() -> SttError {
    SttError::Cancelled("Model pool is shutting down".into())
}

fn capacity_busy() -> SttError {
    SttError::Busy("Model pool capacity is occupied by active work or cleanup".into())
}

fn identity_conflict() -> SttError {
    SttError::Config("Model identity is already bound to another configuration".into())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Barrier,
    };

    use super::*;

    struct FakeLifecycle {
        loads: AtomicUsize,
        retires: AtomicUsize,
        fail_next: AtomicBool,
        cancel_next: AtomicBool,
        busy: AtomicBool,
        cleanup_pending: AtomicBool,
        retire_enters_cleanup: AtomicBool,
        hold_next: AtomicBool,
        load_started: Barrier,
        load_release: Barrier,
    }

    impl Default for FakeLifecycle {
        fn default() -> Self {
            Self {
                loads: AtomicUsize::new(0),
                retires: AtomicUsize::new(0),
                fail_next: AtomicBool::new(false),
                cancel_next: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                cleanup_pending: AtomicBool::new(false),
                retire_enters_cleanup: AtomicBool::new(false),
                hold_next: AtomicBool::new(false),
                load_started: Barrier::new(2),
                load_release: Barrier::new(2),
            }
        }
    }

    impl Lifecycle for FakeLifecycle {
        fn quiescent(&self, _engine: &SttEngine) -> bool {
            !self.busy.load(Ordering::Acquire) && !self.cleanup_pending.load(Ordering::Acquire)
        }

        fn load(
            &self,
            _engine: &SttEngine,
            _selection: &SttModelSelection,
            _acceleration: AccelerationMode,
            cancellation: &OperationCancellation,
        ) -> SttResult<()> {
            self.loads.fetch_add(1, Ordering::AcqRel);
            if self.hold_next.swap(false, Ordering::AcqRel) {
                self.load_started.wait();
                self.load_release.wait();
            }
            if self.cancel_next.swap(false, Ordering::AcqRel) {
                cancellation.cancel();
            }
            if self.fail_next.swap(false, Ordering::AcqRel) {
                Err(SttError::BackendStartFailed(
                    crate::error::RecognitionBackend::Cpu,
                ))
            } else {
                Ok(())
            }
        }

        fn retire(&self, _engine: &SttEngine) -> bool {
            self.retires.fetch_add(1, Ordering::AcqRel);
            if self.retire_enters_cleanup.swap(false, Ordering::AcqRel) {
                self.cleanup_pending.store(true, Ordering::Release);
                return true;
            }
            !self.busy.load(Ordering::Acquire) && !self.cleanup_pending.load(Ordering::Acquire)
        }

        fn cleanup_pending(&self, _engine: &SttEngine) -> bool {
            self.cleanup_pending.load(Ordering::Acquire)
        }
    }

    fn selection(name: &str) -> SttModelSelection {
        SttModelSelection::from_whisper_path(format!("/fixtures/ggml-{name}.bin"))
    }

    fn pool(max: usize, backend: Arc<FakeLifecycle>) -> ModelPool {
        ModelPool::build(SttEnvironment::default(), max, None, backend).unwrap()
    }

    fn acquire(pool: &ModelPool, name: &str, residency: ModelResidency) -> SttResult<ModelLease> {
        pool.acquire(
            &selection(name),
            AccelerationMode::Cpu,
            residency,
            0,
            &OperationCancellation::default(),
        )
    }

    #[test]
    fn two_models_can_remain_loaded_at_once() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(2, backend);
        let first = acquire(&pool, "tiny", ModelResidency::Resident).unwrap();
        let second = acquire(&pool, "base", ModelResidency::Resident).unwrap();
        assert_ne!(first.model_id(), second.model_id());
        assert_eq!(pool.status().loaded, 2);
        assert_eq!(pool.loaded_model_ids().len(), 2);
        drop((first, second));
        assert_eq!(pool.status().loaded, 2);
    }

    #[test]
    fn overlapping_borrowers_do_not_reload_or_replace_one_slot() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(1, Arc::clone(&backend));
        let first = acquire(&pool, "tiny", ModelResidency::Resident).unwrap();
        let second = acquire(&pool, "tiny", ModelResidency::Resident).unwrap();
        assert_eq!(backend.loads.load(Ordering::Acquire), 1);
        drop(second);
        drop(first);

        backend.busy.store(true, Ordering::Release);
        assert!(matches!(
            acquire(&pool, "tiny", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        assert_eq!(backend.loads.load(Ordering::Acquire), 1);
        backend.busy.store(false, Ordering::Release);
        drop(acquire(&pool, "tiny", ModelResidency::Resident).unwrap());
        assert_eq!(backend.loads.load(Ordering::Acquire), 2);
    }

    #[test]
    fn capacity_evicts_least_recently_used_inactive_model() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(2, Arc::clone(&backend));
        drop(acquire(&pool, "tiny", ModelResidency::Resident).unwrap());
        drop(acquire(&pool, "base", ModelResidency::Resident).unwrap());
        drop(acquire(&pool, "tiny", ModelResidency::Resident).unwrap());
        drop(acquire(&pool, "small", ModelResidency::Resident).unwrap());
        let ids = pool.loaded_model_ids();
        assert!(ids.contains(&selection("tiny").id));
        assert!(ids.contains(&selection("small").id));
        assert!(!ids.contains(&selection("base").id));
        assert_eq!(backend.retires.load(Ordering::Acquire), 1);
    }

    #[test]
    fn borrower_and_native_busy_state_prevent_eviction() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(1, Arc::clone(&backend));
        let active = acquire(&pool, "tiny", ModelResidency::Resident).unwrap();
        assert!(matches!(
            acquire(&pool, "base", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        assert_eq!(backend.retires.load(Ordering::Acquire), 0);
        drop(active);
        backend.busy.store(true, Ordering::Release);
        assert!(matches!(
            acquire(&pool, "base", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        assert_eq!(pool.loaded_model_ids(), vec![selection("tiny").id]);
        backend.busy.store(false, Ordering::Release);
        drop(acquire(&pool, "base", ModelResidency::Resident).unwrap());
    }

    #[test]
    fn failed_or_cancelled_load_releases_only_after_cleanup() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(1, Arc::clone(&backend));
        backend.fail_next.store(true, Ordering::Release);
        backend.cleanup_pending.store(true, Ordering::Release);
        assert!(matches!(
            acquire(&pool, "tiny", ModelResidency::Resident),
            Err(SttError::BackendStartFailed(_))
        ));
        assert_eq!(pool.status().retiring, 1);
        assert!(matches!(
            acquire(&pool, "base", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        assert!(matches!(
            acquire(&pool, "tiny", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        backend.cleanup_pending.store(false, Ordering::Release);
        drop(acquire(&pool, "tiny", ModelResidency::Resident).unwrap());
        drop(acquire(&pool, "base", ModelResidency::Resident).unwrap());
        assert_eq!(pool.status().retiring, 0);

        backend.cancel_next.store(true, Ordering::Release);
        drop(
            acquire(&pool, "tiny", ModelResidency::Resident)
                .err()
                .unwrap(),
        );
        assert_eq!(pool.status().loaded, 0);
    }

    #[test]
    fn successful_eviction_keeps_reservation_through_worker_cleanup() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = ModelPool::build(
            SttEnvironment::default(),
            1,
            Some(100),
            Arc::clone(&backend) as Arc<dyn Lifecycle>,
        )
        .unwrap();
        drop(
            pool.acquire(
                &selection("tiny"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                40,
                &OperationCancellation::default(),
            )
            .unwrap(),
        );
        backend.retire_enters_cleanup.store(true, Ordering::Release);
        assert!(matches!(
            pool.acquire(
                &selection("base"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                30,
                &OperationCancellation::default(),
            ),
            Err(SttError::Busy(_))
        ));
        assert_eq!(pool.status().retiring, 1);
        assert_eq!(pool.status().estimated_bytes, 40);
        backend.cleanup_pending.store(false, Ordering::Release);
        drop(
            pool.acquire(
                &selection("base"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                30,
                &OperationCancellation::default(),
            )
            .unwrap(),
        );
        assert_eq!(pool.status().retiring, 0);
        assert_eq!(pool.status().estimated_bytes, 30);
    }

    #[test]
    fn idle_timeout_expires_only_idle_unload_policy() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(2, backend);
        let timed = acquire(&pool, "tiny", ModelResidency::IdleUnload(Duration::ZERO)).unwrap();
        let resident = acquire(&pool, "base", ModelResidency::Resident).unwrap();
        assert_eq!(pool.reap_idle(), 0);
        drop((timed, resident));
        assert_eq!(pool.reap_idle(), 1);
        assert_eq!(pool.loaded_model_ids(), vec![selection("base").id]);
    }

    #[test]
    fn identity_rebinding_is_rejected_even_after_eviction() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(1, backend);
        let custom = SttModelSelection::from_whisper_path("/a/one.bin".into());
        let other = SttModelSelection::from_whisper_path("/b/two.bin".into());
        drop(
            pool.acquire(
                &custom,
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                0,
                &OperationCancellation::default(),
            )
            .unwrap(),
        );
        let conflict = pool.acquire(
            &other,
            AccelerationMode::Cpu,
            ModelResidency::Resident,
            0,
            &OperationCancellation::default(),
        );
        assert!(matches!(conflict, Err(SttError::Config(_))));
    }

    #[test]
    fn estimated_memory_budget_is_admission_only_and_counts_retirement() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = ModelPool::build(
            SttEnvironment::default(),
            2,
            Some(100),
            Arc::clone(&backend) as Arc<dyn Lifecycle>,
        )
        .unwrap();
        assert!(matches!(
            pool.acquire(
                &selection("tiny"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                0,
                &OperationCancellation::default()
            ),
            Err(SttError::Config(_))
        ));
        drop(
            pool.acquire(
                &selection("tiny"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                60,
                &OperationCancellation::default(),
            )
            .unwrap(),
        );
        drop(
            pool.acquire(
                &selection("base"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                50,
                &OperationCancellation::default(),
            )
            .unwrap(),
        );
        assert_eq!(pool.status().estimated_bytes, 50);
        assert_eq!(pool.loaded_model_ids(), vec![selection("base").id]);
    }

    #[test]
    fn loading_reservation_blocks_overcommit_and_waiter_can_cancel() {
        let backend = Arc::new(FakeLifecycle::default());
        backend.hold_next.store(true, Ordering::Release);
        let pool = pool(1, Arc::clone(&backend));
        let loading_pool = pool.clone();
        let loading = std::thread::spawn(move || {
            acquire(&loading_pool, "tiny", ModelResidency::Resident).unwrap()
        });
        backend.load_started.wait();
        assert_eq!(pool.status().loading, 1);
        let waiting_pool = pool.clone();
        let cancellation = OperationCancellation::default();
        let waiter_cancel = cancellation.clone();
        let waiting = std::thread::spawn(move || {
            waiting_pool.acquire(
                &selection("base"),
                AccelerationMode::Cpu,
                ModelResidency::Resident,
                0,
                &waiter_cancel,
            )
        });
        std::thread::sleep(Duration::from_millis(60));
        cancellation.cancel();
        assert!(matches!(
            waiting.join().unwrap(),
            Err(SttError::Cancelled(_))
        ));
        assert_eq!(pool.status().loading, 1);
        backend.load_release.wait();
        let lease = loading.join().unwrap();
        assert!(matches!(
            acquire(&pool, "base", ModelResidency::Resident),
            Err(SttError::Busy(_))
        ));
        drop(lease);
        assert_eq!(pool.status().loaded, 1);
    }

    #[test]
    fn shutdown_retires_resident_models_and_rejects_new_acquisitions() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(2, Arc::clone(&backend));
        drop(acquire(&pool, "tiny", ModelResidency::Resident).unwrap());
        drop(acquire(&pool, "base", ModelResidency::Resident).unwrap());
        assert_eq!(pool.status().loaded, 2);

        pool.begin_shutdown();
        assert!(matches!(
            acquire(&pool, "tiny", ModelResidency::Resident),
            Err(SttError::Cancelled(_))
        ));
        assert_eq!(pool.reap_shutdown().loaded, 0);
        assert!(pool.shutdown_complete());
        assert_eq!(backend.retires.load(Ordering::Acquire), 2);
    }

    #[test]
    fn shutdown_waits_for_last_borrower_and_supervised_cleanup() {
        let backend = Arc::new(FakeLifecycle::default());
        let pool = pool(1, Arc::clone(&backend));
        let lease = acquire(&pool, "tiny", ModelResidency::Resident).unwrap();
        pool.begin_shutdown();
        assert_eq!(pool.reap_shutdown().loaded, 1);
        assert!(!pool.shutdown_complete());
        assert_eq!(backend.retires.load(Ordering::Acquire), 0);

        drop(lease);
        backend.retire_enters_cleanup.store(true, Ordering::Release);
        assert_eq!(pool.reap_shutdown().retiring, 1);
        assert!(!pool.shutdown_complete());
        backend.cleanup_pending.store(false, Ordering::Release);
        assert_eq!(pool.reap_shutdown().retiring, 0);
        assert!(pool.shutdown_complete());
    }

    #[test]
    fn shutdown_during_load_retires_it_without_publishing_a_lease() {
        let backend = Arc::new(FakeLifecycle::default());
        backend.hold_next.store(true, Ordering::Release);
        let pool = pool(1, Arc::clone(&backend));
        let loading_pool = pool.clone();
        let loading =
            std::thread::spawn(move || acquire(&loading_pool, "tiny", ModelResidency::Resident));
        backend.load_started.wait();
        assert_eq!(pool.status().loading, 1);
        pool.begin_shutdown();
        assert!(!pool.shutdown_complete());
        backend.load_release.wait();
        assert!(matches!(
            loading.join().unwrap(),
            Err(SttError::Cancelled(_))
        ));
        assert!(pool.shutdown_complete());
        assert_eq!(backend.retires.load(Ordering::Acquire), 1);
    }
}
