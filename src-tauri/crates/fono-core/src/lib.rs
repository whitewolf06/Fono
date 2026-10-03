//! Tauri-independent operation FSM, ownership and cancellation policies.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use thiserror::Error;

/// Platform-neutral capture boundary. The application selects the buffer and
/// error types; CPAL/Tauri stay in the adapter crate.
pub trait AudioCapturePort<Writer, Error>: Send + Sync {
    /// Optional sample-clock handoff. Adapters that have no continuous audio
    /// source use the ordinary start implementation.
    fn start_after(
        &self,
        device_id: Option<&str>,
        writer: Writer,
        limit_reached: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
        maximum_samples: usize,
        _cursor: Option<(u64, u64)>,
    ) -> Result<(), Error> {
        self.start(
            device_id,
            writer,
            limit_reached,
            level_bits,
            maximum_samples,
        )
    }

    fn start(
        &self,
        device_id: Option<&str>,
        writer: Writer,
        limit_reached: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
        maximum_samples: usize,
    ) -> Result<(), Error>;

    fn stop(&self) -> Result<(), Error>;

    fn shutdown(&self);
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoordinatorError {
    #[error("operation is busy: {0}")]
    Busy(String),
    #[error("operation {0} is no longer active")]
    StaleOperation(u64),
    #[error("resource {resource:?} is already leased by operation {operation_id}")]
    ResourceAlreadyLeased {
        operation_id: u64,
        resource: OperationResource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationResource {
    Audio,
    Stt,
    Injection,
    CommandProposal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationSource {
    Ui,
    Hotkey,
    WakeWord,
    Diagnostics,
    /// A non-interactive request submitted through the local service layer.
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Recording,
    Transcribing,
    Processing,
    Injecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalReason {
    Completed,
    Cancelled,
    Failed,
    RecordingLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationSnapshot {
    pub id: u64,
    pub source: OperationSource,
    pub phase: OperationPhase,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalOperation {
    pub id: u64,
    pub source: OperationSource,
    pub phase: OperationPhase,
    pub reason: TerminalReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationEvent {
    PhaseChanged(OperationSnapshot),
    Finished(TerminalOperation),
}

#[derive(Default)]
pub struct OperationCoordinator {
    inner: Mutex<CoordinatorState>,
}

#[derive(Default)]
struct CoordinatorState {
    next_id: u64,
    active: Option<ActiveOperation>,
}

struct ActiveOperation {
    snapshot: OperationSnapshot,
    cancellation: OperationCancellation,
    resources: BTreeSet<OperationResource>,
}

#[derive(Clone, Default)]
pub struct OperationCancellation {
    cancelled: Arc<AtomicBool>,
    linked: Arc<[Arc<AtomicBool>]>,
}

impl OperationCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .linked
                .iter()
                .any(|signal| signal.load(Ordering::Acquire))
    }

    /// Allows a higher-level scheduler to propagate a user-requested cancel to
    /// its active STT operation without owning the coordinator itself.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Creates a cancellation view that stops when either source is cancelled.
    /// The returned signal may still be cancelled independently by its owner.
    pub fn combined_with(&self, other: &Self) -> Self {
        let mut linked = Vec::with_capacity(2 + self.linked.len() + other.linked.len());
        linked.push(Arc::clone(&self.cancelled));
        linked.extend(self.linked.iter().cloned());
        linked.push(Arc::clone(&other.cancelled));
        linked.extend(other.linked.iter().cloned());
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            linked: linked.into(),
        }
    }
}

impl OperationCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self, source: OperationSource) -> Result<OperationSnapshot, CoordinatorError> {
        let mut state = self.inner.lock();
        if let Some(active) = state.active.as_ref() {
            return Err(CoordinatorError::Busy(format!(
                "operation {} from {:?} is still {:?}",
                active.snapshot.id, active.snapshot.source, active.snapshot.phase
            )));
        }
        state.next_id = state.next_id.wrapping_add(1);
        let operation = OperationSnapshot {
            id: state.next_id,
            source,
            phase: OperationPhase::Recording,
            confirmed: false,
        };
        state.active = Some(ActiveOperation {
            snapshot: operation.clone(),
            cancellation: OperationCancellation::default(),
            resources: BTreeSet::new(),
        });
        Ok(operation)
    }

    pub fn current(&self) -> Option<OperationSnapshot> {
        self.inner
            .lock()
            .active
            .as_ref()
            .map(|active| active.snapshot.clone())
    }

    pub fn is_active(&self, operation_id: u64) -> bool {
        self.inner
            .lock()
            .active
            .as_ref()
            .is_some_and(|operation| operation.snapshot.id == operation_id)
    }

    pub fn cancellation(&self, operation_id: u64) -> Option<OperationCancellation> {
        self.inner
            .lock()
            .active
            .as_ref()
            .filter(|operation| operation.snapshot.id == operation_id)
            .map(|operation| operation.cancellation.clone())
    }

    pub fn acquire_resource(
        &self,
        operation_id: u64,
        resource: OperationResource,
    ) -> Result<(), CoordinatorError> {
        let mut state = self.inner.lock();
        let active = state
            .active
            .as_mut()
            .filter(|active| active.snapshot.id == operation_id)
            .ok_or(CoordinatorError::StaleOperation(operation_id))?;
        if !active.resources.insert(resource) {
            return Err(CoordinatorError::ResourceAlreadyLeased {
                operation_id,
                resource,
            });
        }
        Ok(())
    }

    pub fn release_resource(&self, operation_id: u64, resource: OperationResource) -> bool {
        let mut state = self.inner.lock();
        state
            .active
            .as_mut()
            .filter(|active| active.snapshot.id == operation_id)
            .is_some_and(|active| active.resources.remove(&resource))
    }

    pub fn resources(&self, operation_id: u64) -> Option<Vec<OperationResource>> {
        self.inner
            .lock()
            .active
            .as_ref()
            .filter(|active| active.snapshot.id == operation_id)
            .map(|active| active.resources.iter().copied().collect())
    }

    pub fn transition(&self, operation_id: u64, phase: OperationPhase) -> Option<OperationEvent> {
        let mut state = self.inner.lock();
        let active = state.active.as_mut()?;
        if active.snapshot.id != operation_id || !can_transition(active.snapshot.phase, phase) {
            return None;
        }
        active.snapshot.phase = phase;
        Some(OperationEvent::PhaseChanged(active.snapshot.clone()))
    }

    pub fn confirm(&self, operation_id: u64) -> Option<OperationSnapshot> {
        let mut state = self.inner.lock();
        let active = state.active.as_mut()?;
        if active.snapshot.id != operation_id {
            return None;
        }
        active.snapshot.confirmed = true;
        Some(active.snapshot.clone())
    }

    pub fn finish(&self, operation_id: u64, reason: TerminalReason) -> Option<OperationEvent> {
        let mut state = self.inner.lock();
        let active = state.active.take()?;
        if active.snapshot.id != operation_id {
            state.active = Some(active);
            return None;
        }
        Some(OperationEvent::Finished(TerminalOperation {
            id: active.snapshot.id,
            source: active.snapshot.source,
            phase: active.snapshot.phase,
            reason,
        }))
    }

    pub fn cancel(&self, operation_id: u64) -> Option<OperationEvent> {
        let cancellation = self.cancellation(operation_id)?;
        cancellation.cancel();
        self.finish(operation_id, TerminalReason::Cancelled)
    }
}

fn can_transition(from: OperationPhase, to: OperationPhase) -> bool {
    from == to
        || matches!(
            (from, to),
            (OperationPhase::Recording, OperationPhase::Transcribing)
                | (OperationPhase::Transcribing, OperationPhase::Processing)
                | (OperationPhase::Transcribing, OperationPhase::Injecting)
                | (OperationPhase::Processing, OperationPhase::Injecting)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_operation_owns_the_lease() {
        let coordinator = OperationCoordinator::new();
        let first = coordinator.start(OperationSource::Ui).unwrap();
        assert!(matches!(
            coordinator.start(OperationSource::Hotkey),
            Err(CoordinatorError::Busy(_))
        ));
        coordinator.finish(first.id, TerminalReason::Completed);
        assert!(coordinator.start(OperationSource::Hotkey).unwrap().id > first.id);
    }

    #[test]
    fn service_requests_share_the_same_operation_lease() {
        let coordinator = OperationCoordinator::new();
        let service = coordinator.start(OperationSource::Service).unwrap();

        assert!(matches!(
            coordinator.start(OperationSource::Ui),
            Err(CoordinatorError::Busy(_))
        ));

        coordinator
            .finish(service.id, TerminalReason::Completed)
            .unwrap();
        assert!(coordinator.start(OperationSource::Ui).is_ok());
    }

    #[test]
    fn legal_and_illegal_transitions_are_enforced() {
        let coordinator = OperationCoordinator::new();
        let operation = coordinator.start(OperationSource::Ui).unwrap();
        assert!(coordinator
            .transition(operation.id, OperationPhase::Processing)
            .is_none());
        assert!(coordinator
            .transition(operation.id, OperationPhase::Transcribing)
            .is_some());
        assert!(coordinator
            .transition(operation.id, OperationPhase::Processing)
            .is_some());
        assert!(coordinator
            .transition(operation.id, OperationPhase::Injecting)
            .is_some());
    }

    #[test]
    fn stale_operation_cannot_mutate_the_current_one() {
        let coordinator = OperationCoordinator::new();
        let stale = coordinator.start(OperationSource::Ui).unwrap();
        coordinator.cancel(stale.id);
        let current = coordinator.start(OperationSource::WakeWord).unwrap();
        assert!(coordinator
            .transition(stale.id, OperationPhase::Transcribing)
            .is_none());
        assert_eq!(coordinator.current(), Some(current));
    }

    #[test]
    fn cancellation_signal_survives_terminal_removal() {
        let coordinator = OperationCoordinator::new();
        let operation = coordinator.start(OperationSource::Ui).unwrap();
        let cancellation = coordinator.cancellation(operation.id).unwrap();
        coordinator.cancel(operation.id).unwrap();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn combined_cancellation_observes_either_owner() {
        let first = OperationCancellation::default();
        let second = OperationCancellation::default();
        let combined = first.combined_with(&second);

        second.cancel();

        assert!(combined.is_cancelled());
    }

    #[test]
    fn resource_leases_are_operation_scoped_and_cleared_by_terminal_failure() {
        let coordinator = OperationCoordinator::new();
        let operation = coordinator.start(OperationSource::Ui).unwrap();

        coordinator
            .acquire_resource(operation.id, OperationResource::Audio)
            .unwrap();
        assert!(matches!(
            coordinator.acquire_resource(operation.id, OperationResource::Audio),
            Err(CoordinatorError::ResourceAlreadyLeased { .. })
        ));
        assert_eq!(
            coordinator.resources(operation.id),
            Some(vec![OperationResource::Audio])
        );

        coordinator
            .finish(operation.id, TerminalReason::Failed)
            .unwrap();
        assert!(coordinator.resources(operation.id).is_none());
        assert!(!coordinator.release_resource(operation.id, OperationResource::Audio));
    }

    #[test]
    fn one_thousand_terminal_cycles_leave_no_active_operation_or_resource_leases() {
        let coordinator = OperationCoordinator::new();

        for index in 0..1_000 {
            let operation = coordinator.start(OperationSource::Diagnostics).unwrap();
            coordinator
                .acquire_resource(operation.id, OperationResource::Audio)
                .unwrap();
            coordinator
                .acquire_resource(operation.id, OperationResource::Stt)
                .unwrap();
            coordinator
                .acquire_resource(operation.id, OperationResource::Injection)
                .unwrap();

            let terminal_event = if index % 2 == 0 {
                coordinator.cancel(operation.id).unwrap()
            } else {
                coordinator
                    .finish(operation.id, TerminalReason::Failed)
                    .unwrap()
            };

            assert!(matches!(
                terminal_event,
                OperationEvent::Finished(finished) if finished.id == operation.id
            ));
            assert!(coordinator.current().is_none());
            assert!(coordinator.resources(operation.id).is_none());
        }
    }

    #[test]
    fn stale_operation_cannot_acquire_or_release_a_resource() {
        let coordinator = OperationCoordinator::new();
        let stale = coordinator.start(OperationSource::Ui).unwrap();
        coordinator.cancel(stale.id).unwrap();
        let current = coordinator.start(OperationSource::Hotkey).unwrap();

        assert_eq!(
            coordinator.acquire_resource(stale.id, OperationResource::Audio),
            Err(CoordinatorError::StaleOperation(stale.id))
        );
        assert!(!coordinator.release_resource(stale.id, OperationResource::Audio));
        assert_eq!(coordinator.resources(current.id), Some(Vec::new()));
    }
}
