//! Crash-only bounded worker recovery. Deliberate shutdown/cancel/detach never
//! emits a fault; stale generations and selections cannot publish a reload.
use super::*;
use crate::application::updates::activity::{self, ActivityLease};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock, Weak,
    },
    time::{Duration, Instant},
};

pub(super) type FaultObserver = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy)]
pub(super) struct LoadFence<'a> {
    pub generation: u64,
    pub valid: &'a dyn Fn(u64) -> bool,
}

#[derive(Default)]
pub(super) struct FaultSignal {
    faulted: AtomicBool,
    suppressed: AtomicBool,
    emitted: AtomicBool,
    observer: Mutex<Option<FaultObserver>>,
}

impl FaultSignal {
    pub fn fault(&self) {
        self.faulted.store(true, Ordering::Release);
        self.dispatch();
    }
    pub fn suppress(&self) {
        self.suppressed.store(true, Ordering::Release);
    }
    pub fn is_faulted(&self) -> bool {
        self.faulted.load(Ordering::Acquire)
    }
    pub fn arm(&self, observer: FaultObserver) {
        *self.observer.lock() = Some(observer);
        self.dispatch();
    }
    fn dispatch(&self) {
        if !self.faulted.load(Ordering::Acquire) || self.suppressed.load(Ordering::Acquire) {
            return;
        }
        let Some(observer) = self.observer.lock().clone() else {
            return;
        };
        if self
            .emitted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && !self.suppressed.load(Ordering::Acquire)
        {
            observer();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoverySelection {
    pub model_path: PathBuf,
    pub acceleration: AccelerationMode,
    pub worker_paths: WorkerPaths,
}
pub type RecoverySelectionProvider = Arc<dyn Fn() -> Option<RecoverySelection> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct PreparedRecoverySelection {
    generation: u64,
    selection: RecoverySelection,
}
struct PublishedSelection {
    prepared: PreparedRecoverySelection,
    faulted: Arc<AtomicBool>,
}

struct Fault {
    generation: u64,
    selection: RecoverySelection,
    _activity: ActivityLease<'static>,
    manual_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FaultKey {
    generation: u64,
    manual_revision: u64,
}

struct LatestFault<T> {
    pending: Option<(FaultKey, T)>,
    high_water: Option<FaultKey>,
}
impl<T> Default for LatestFault<T> {
    fn default() -> Self {
        Self {
            pending: None,
            high_water: None,
        }
    }
}
impl<T> LatestFault<T> {
    /// Return displaced/rejected work to the caller, so admission leases are
    /// always dropped after the coalescing mutex has been released.
    fn offer(&mut self, key: FaultKey, item: T) -> Result<Option<T>, T> {
        if self.high_water.is_some_and(|latest| latest >= key) {
            return Err(item);
        }
        self.high_water = Some(key);
        Ok(self.pending.replace((key, item)).map(|(_, item)| item))
    }
    fn take(&mut self) -> Option<T> {
        self.pending.take().map(|(_, item)| item)
    }
}
enum Message {
    Wake,
    Stop,
}

#[derive(Default)]
pub(super) struct RecoveryControl {
    sender: OnceLock<Sender<Message>>,
    latest: Mutex<LatestFault<Fault>>,
    stopped: AtomicBool,
    registration: Mutex<()>,
    manual_revision: std::sync::atomic::AtomicU64,
    publication: Mutex<()>,
    published: Mutex<Option<PublishedSelection>>,
}

#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub backoff: [Duration; 3],
    pub stable_window: Duration,
}
const TIMING: Timing = Timing {
    backoff: [
        Duration::from_millis(250),
        Duration::from_secs(1),
        Duration::from_secs(3),
    ],
    stable_window: Duration::from_secs(60),
};
const FAILED: &str = "Не удалось восстановить распознавание после трёх сбоев. Проверьте ускорение или выберите модель заново";
const CLEANUP_FAILED: &str =
    "Не удалось завершить предыдущий процесс распознавания. Перезапустите Fono";
const CLEANUP_WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Policy {
    selection: Option<RecoverySelection>,
    last_fault: Option<u64>,
    attempts: usize,
    ready_since: Option<Instant>,
    manual_revision: u64,
}
impl Policy {
    fn accept(&mut self, fault: &Fault, now: Instant, timing: Timing) -> bool {
        if self.last_fault == Some(fault.generation)
            && self.selection.as_ref() == Some(&fault.selection)
            && self.manual_revision == fault.manual_revision
        {
            return false;
        }
        if self.selection.as_ref() != Some(&fault.selection)
            || self.manual_revision != fault.manual_revision
            || self
                .ready_since
                .is_some_and(|ready| now.saturating_duration_since(ready) >= timing.stable_window)
        {
            self.attempts = 0;
        }
        self.selection = Some(fault.selection.clone());
        self.manual_revision = fault.manual_revision;
        self.last_fault = Some(fault.generation);
        self.ready_since = None;
        true
    }
    fn attempt(&mut self, timing: Timing) -> Option<Duration> {
        let delay = timing.backoff.get(self.attempts).copied()?;
        self.attempts += 1;
        Some(delay)
    }
}

impl SttEngine {
    pub fn enable_recovery(
        self: &Arc<Self>,
        selection: RecoverySelectionProvider,
    ) -> AppResult<()> {
        self.enable_recovery_with_timing(selection, TIMING)
    }
    pub(super) fn enable_recovery_with_timing(
        self: &Arc<Self>,
        selection: RecoverySelectionProvider,
        timing: Timing,
    ) -> AppResult<()> {
        let (tx, rx) = bounded(1);
        let _registration = self.recovery.registration.lock();
        if self.recovery.stopped.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("STT recovery is shutting down".into()));
        }
        if self.recovery.sender.get().is_some() {
            return Ok(());
        }
        let engine = Arc::downgrade(self);
        let control = self.recovery.clone();
        std::thread::Builder::new()
            .name("fono-stt-recovery".into())
            .spawn(move || supervise(engine, control, rx, selection, timing))
            .map_err(|_| AppError::Internal("STT recovery owner could not be created".into()))?;
        let _ = self.recovery.sender.set(tx);
        Ok(())
    }
    pub(crate) fn stop_recovery(&self) {
        {
            let _publication = self.recovery.publication.lock();
            self.recovery.stopped.store(true, Ordering::Release);
        }
        let discarded = self.recovery.latest.lock().take();
        drop(discarded);
        if let Some(sender) = self.recovery.sender.get() {
            let _ = sender.try_send(Message::Stop);
        }
    }
    /// Only explicit model/acceleration selection resets the bounded crash
    /// budget. Ordinary ensure_loaded calls during dictation never reset it.
    pub(crate) fn reset_recovery_budget(&self) {
        let _publication = self.recovery.publication.lock();
        self.recovery.manual_revision.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn prepared_recovery_selection(
        &self,
        selection: &RecoverySelection,
    ) -> Option<PreparedRecoverySelection> {
        let _publication = self.recovery_publication();
        self.recovery
            .published
            .lock()
            .as_ref()
            .and_then(|published| {
                (published.prepared.generation == self.generation.load(Ordering::Acquire)
                    && &published.prepared.selection == selection)
                    .then(|| published.prepared.clone())
            })
    }
    /// The native prepare precedes persistence. A fault in that gap is latched
    /// but cannot reload settings that have not been committed. Only the exact
    /// successful transaction may acknowledge and requeue its current fault.
    pub(crate) fn acknowledge_recovery_selection(
        &self,
        prepared: PreparedRecoverySelection,
    ) -> bool {
        let Ok(activity) = activity::lease() else {
            return false;
        };
        let _publication = self.recovery_publication();
        if self.recovery.stopped.load(Ordering::Acquire)
            || self.recovery.sender.get().is_none()
            || self.generation.load(Ordering::Acquire) != prepared.generation
        {
            return false;
        }
        let faulted = {
            let published = self.recovery.published.lock();
            let Some(published) = published.as_ref() else {
                return false;
            };
            if published.prepared.generation != prepared.generation
                || published.prepared.selection != prepared.selection
            {
                return false;
            }
            published.faulted.load(Ordering::Acquire)
        };
        // A successful explicit selection is a new manual revision, including
        // when a same-generation fault was already rejected before persistence.
        let revision = self.recovery.manual_revision.fetch_add(1, Ordering::AcqRel) + 1;
        if faulted {
            queue_fault(
                &self.recovery,
                prepared.generation,
                prepared.selection,
                revision,
                activity,
            );
        }
        true
    }
    pub(super) fn recovery_publication(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.recovery.publication.lock()
    }

    pub(super) fn load_readiness(
        &self,
        readiness: SttReadiness,
        fence: Option<LoadFence<'_>>,
        generation: u64,
    ) -> AppResult<()> {
        let _publication = self.recovery_publication();
        if fence.is_some_and(|fence| !(fence.valid)(generation)) {
            return Err(AppError::Cancelled(
                "STT readiness belongs to a stale recovery".into(),
            ));
        }
        if matches!(readiness, SttReadiness::Ready { .. }) {
            let state = self.state.lock();
            let active = state.active();
            if active.is_none() || active.is_some_and(|active| active.worker_faulted()) {
                return Err(AppError::Cancelled(
                    "STT backend retired before readiness publication".into(),
                ));
            }
        }
        self.set_readiness(readiness);
        Ok(())
    }

    pub(super) fn arm_worker_recovery(&self, generation: u64, selection: RecoverySelection) {
        let faulted = Arc::new(AtomicBool::new(false));
        *self.recovery.published.lock() = Some(PublishedSelection {
            prepared: PreparedRecoverySelection {
                generation,
                selection: selection.clone(),
            },
            faulted: faulted.clone(),
        });
        #[cfg(test)]
        {
            let before_arm = self.before_recovery_arm.lock().take();
            if let Some(before_arm) = before_arm {
                before_arm();
            }
        }
        let mailbox = match &*self.state.lock() {
            EngineState::Worker { session, .. } => session.clone(),
            _ => return,
        };
        let control = self.recovery.clone();
        mailbox.arm_fault(Arc::new(move || {
            faulted.store(true, Ordering::Release);
            if control.stopped.load(Ordering::Acquire) {
                return;
            }
            if control.sender.get().is_none() {
                return;
            }
            let Ok(activity) = activity::lease() else {
                return;
            };
            // The owner callback does no loading, settings I/O or native teardown.
            let manual_revision = control.manual_revision.load(Ordering::Acquire);
            queue_fault(
                &control,
                generation,
                selection.clone(),
                manual_revision,
                activity,
            );
        }));
    }
}

fn queue_fault(
    control: &RecoveryControl,
    generation: u64,
    selection: RecoverySelection,
    manual_revision: u64,
    activity: ActivityLease<'static>,
) {
    let mut latest = control.latest.lock();
    if control.stopped.load(Ordering::Acquire) {
        return;
    }
    let discarded = latest.offer(
        FaultKey {
            generation,
            manual_revision,
        },
        Fault {
            generation,
            selection,
            _activity: activity,
            manual_revision,
        },
    );
    let queued = discarded.is_ok();
    drop(latest);
    drop(discarded);
    if queued {
        if let Some(sender) = control.sender.get() {
            let _ = sender.try_send(Message::Wake);
        }
    }
}

fn supervise(
    engine: Weak<SttEngine>,
    control: Arc<RecoveryControl>,
    receiver: Receiver<Message>,
    selection: RecoverySelectionProvider,
    timing: Timing,
) {
    let mut policy = Policy::default();
    while !control.stopped.load(Ordering::Acquire) {
        let fault = match receiver.recv() {
            Ok(Message::Wake) => match control.latest.lock().take() {
                Some(fault) => fault,
                None => continue,
            },
            Ok(Message::Stop) => break,
            Err(_) => break,
        };
        let Some(engine) = engine.upgrade() else {
            break;
        };
        let generation_valid = |expected| {
            !control.stopped.load(Ordering::Acquire)
                && engine.generation.load(Ordering::Acquire) == expected
                && control.manual_revision.load(Ordering::Acquire) == fault.manual_revision
        };
        let selected_generation =
            |expected| generation_valid(expected) && selection().as_ref() == Some(&fault.selection);
        let valid = || selected_generation(fault.generation);
        let fence = LoadFence {
            generation: fault.generation,
            valid: &selected_generation,
        };
        if !valid() || !policy.accept(&fault, Instant::now(), timing) {
            continue;
        }
        loop {
            // Resource cleanup is not a load attempt. In particular, do not
            // exhaust the crash budget while stdio/native teardown is pending.
            let cleanup_deadline = Instant::now() + CLEANUP_WAIT;
            while engine.cleanup.is_pending() && valid() && Instant::now() < cleanup_deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            if !valid() {
                break;
            }
            if engine.cleanup.is_pending() {
                let _ = engine.load_readiness(
                    SttReadiness::Failed {
                        message: CLEANUP_FAILED.into(),
                    },
                    Some(fence),
                    fault.generation,
                );
                break;
            }
            let Some(delay) = policy.attempt(timing) else {
                if valid() {
                    let _ = engine.load_readiness(
                        SttReadiness::Failed {
                            message: FAILED.into(),
                        },
                        Some(fence),
                        fault.generation,
                    );
                }
                break;
            };
            // A dedicated blocking owner sleeps; UI and async runtime stay free.
            // Wake in small bounds to honor shutdown/new selections promptly.
            let deadline = Instant::now() + delay;
            while generation_valid(fault.generation) && Instant::now() < deadline {
                std::thread::sleep(
                    Duration::from_millis(25)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            if !valid() {
                break;
            }
            if engine.cleanup.is_pending() {
                policy.attempts = policy.attempts.saturating_sub(1);
                continue;
            }
            engine.reap_finished_worker();
            let result = engine.ensure_loaded_if(
                &fault.selection.model_path,
                fault.selection.acceleration,
                &fault.selection.worker_paths,
                Some(fence),
            );
            match result {
                Ok(()) => {
                    policy.ready_since = Some(Instant::now());
                    break;
                }
                Err(AppError::Cancelled(_)) => break,
                Err(AppError::Busy(_)) if engine.cleanup.is_pending() => {
                    policy.attempts = policy.attempts.saturating_sub(1);
                }
                Err(_) => continue,
            }
        }
    }
}

impl Drop for SttEngine {
    fn drop(&mut self) {
        self.stop_recovery();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_fault_never_loses_new_generation_to_a_late_old_callback() {
        let mut slot = LatestFault::default();
        let new = FaultKey {
            generation: 8,
            manual_revision: 2,
        };
        let old = FaultKey {
            generation: 7,
            manual_revision: 99,
        };
        assert!(slot.offer(new, "new current worker").is_ok());
        assert_eq!(slot.offer(old, "late old worker"), Err("late old worker"));
        assert_eq!(slot.take(), Some("new current worker"));
        assert_eq!(slot.offer(old, "old after Wake"), Err("old after Wake"));
        let reset = FaultKey {
            generation: 8,
            manual_revision: 3,
        };
        assert!(slot.offer(reset, "explicit manual revision").is_ok());
        assert_eq!(slot.take(), Some("explicit manual revision"));
    }
    #[test]
    fn prepublication_fault_is_delivered_once_after_arming() {
        let signal = FaultSignal::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        signal.fault();
        let observed = calls.clone();
        signal.arm(Arc::new(move || {
            observed.fetch_add(1, Ordering::AcqRel);
        }));
        signal.fault();
        assert_eq!(calls.load(Ordering::Acquire), 1);
    }
    #[test]
    fn deliberate_shutdown_cancel_and_adaptive_release_emit_no_fault() {
        for _reason in ["shutdown", "cancel", "adaptive", "replacement"] {
            let signal = FaultSignal::default();
            signal.suppress();
            signal.arm(Arc::new(|| panic!("planned retirement emitted a crash")));
            signal.fault();
        }
    }
    #[test]
    fn repeated_crashes_stop_at_three_attempts_and_stable_window_resets_budget() {
        let selection = RecoverySelection {
            model_path: "fixture.bin".into(),
            acceleration: AccelerationMode::Cuda,
            worker_paths: WorkerPaths::default(),
        };
        let mut policy = Policy::default();
        let fault = Fault {
            generation: 1,
            selection: selection.clone(),
            _activity: activity::lease().unwrap(),
            manual_revision: 0,
        };
        let now = Instant::now();
        assert!(policy.accept(&fault, now, TIMING));
        assert!(!policy.accept(&fault, now, TIMING));
        for expected in TIMING.backoff {
            assert_eq!(policy.attempt(TIMING), Some(expected));
        }
        assert_eq!(policy.attempt(TIMING), None);
        policy.ready_since = Some(now);
        let next = Fault {
            generation: 2,
            selection,
            _activity: activity::lease().unwrap(),
            manual_revision: 0,
        };
        assert!(policy.accept(&next, now + TIMING.stable_window, TIMING));
        assert_eq!(policy.attempt(TIMING), Some(TIMING.backoff[0]));
    }
}
