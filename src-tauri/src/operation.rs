//! Ownership and lifecycle of a single user-visible voice operation.
//!
//! The coordinator is intentionally independent from Tauri, audio and STT. It
//! is the narrow concurrency boundary that every UI, hotkey, wake-word and
//! diagnostics entry point will use before acquiring a recording resource.

use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationSource {
    Ui,
    Hotkey,
    WakeWord,
    Diagnostics,
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
    active: Option<OperationSnapshot>,
}

impl OperationCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claims the sole operation lease. The lease is retained until a matching
    /// call to `finish` or `cancel`; concurrent starts receive `AppError::Busy`.
    pub fn start(&self, source: OperationSource) -> AppResult<OperationSnapshot> {
        let mut state = self.inner.lock();
        if let Some(active) = state.active.as_ref() {
            return Err(AppError::Busy(format!(
                "operation {} from {:?} is still {:?}",
                active.id, active.source, active.phase
            )));
        }

        state.next_id = state.next_id.wrapping_add(1);
        let operation = OperationSnapshot {
            id: state.next_id,
            source,
            phase: OperationPhase::Recording,
            confirmed: false,
        };
        state.active = Some(operation.clone());
        Ok(operation)
    }

    pub fn current(&self) -> Option<OperationSnapshot> {
        self.inner.lock().active.clone()
    }

    pub fn is_active(&self, operation_id: u64) -> bool {
        self.inner
            .lock()
            .active
            .as_ref()
            .is_some_and(|operation| operation.id == operation_id)
    }

    /// Moves an operation through the legal processing phases. A late result
    /// from a cancelled or replaced operation is ignored rather than changing
    /// the state of the current one.
    pub fn transition(&self, operation_id: u64, phase: OperationPhase) -> Option<OperationEvent> {
        let mut state = self.inner.lock();
        let active = state.active.as_mut()?;
        if active.id != operation_id || !can_transition(active.phase, phase) {
            return None;
        }
        active.phase = phase;
        Some(OperationEvent::PhaseChanged(active.clone()))
    }

    pub fn confirm(&self, operation_id: u64) -> Option<OperationSnapshot> {
        let mut state = self.inner.lock();
        let active = state.active.as_mut()?;
        if active.id != operation_id {
            return None;
        }
        active.confirmed = true;
        Some(active.clone())
    }

    pub fn finish(&self, operation_id: u64, reason: TerminalReason) -> Option<OperationEvent> {
        let mut state = self.inner.lock();
        let active = state.active.take()?;
        if active.id != operation_id {
            state.active = Some(active);
            return None;
        }
        Some(OperationEvent::Finished(TerminalOperation {
            id: active.id,
            source: active.source,
            phase: active.phase,
            reason,
        }))
    }

    pub fn cancel(&self, operation_id: u64) -> Option<OperationEvent> {
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
    fn only_one_operation_can_own_the_lease() {
        let coordinator = OperationCoordinator::new();
        let first = coordinator.start(OperationSource::Ui).unwrap();

        assert!(matches!(
            coordinator.start(OperationSource::Hotkey),
            Err(AppError::Busy(_))
        ));
        coordinator.finish(first.id, TerminalReason::Completed);

        let next = coordinator.start(OperationSource::Hotkey).unwrap();
        assert!(next.id > first.id);
        assert_eq!(next.source, OperationSource::Hotkey);
    }

    #[test]
    fn phase_transitions_are_table_driven() {
        let cases = [
            (OperationPhase::Recording, OperationPhase::Recording, true),
            (
                OperationPhase::Recording,
                OperationPhase::Transcribing,
                true,
            ),
            (
                OperationPhase::Transcribing,
                OperationPhase::Processing,
                true,
            ),
            (
                OperationPhase::Transcribing,
                OperationPhase::Injecting,
                true,
            ),
            (OperationPhase::Processing, OperationPhase::Injecting, true),
            (OperationPhase::Recording, OperationPhase::Processing, false),
            (
                OperationPhase::Processing,
                OperationPhase::Transcribing,
                false,
            ),
            (OperationPhase::Injecting, OperationPhase::Recording, false),
        ];

        for (from, to, expected) in cases {
            let coordinator = OperationCoordinator::new();
            let operation = coordinator.start(OperationSource::Ui).unwrap();
            advance_to(&coordinator, operation.id, from);

            assert_eq!(coordinator.transition(operation.id, to).is_some(), expected);
        }
    }

    #[test]
    fn late_operation_cannot_change_or_finish_the_current_one() {
        let coordinator = OperationCoordinator::new();
        let first = coordinator.start(OperationSource::Ui).unwrap();
        coordinator.cancel(first.id);
        let current = coordinator.start(OperationSource::WakeWord).unwrap();

        assert!(coordinator
            .transition(first.id, OperationPhase::Transcribing)
            .is_none());
        assert!(coordinator
            .finish(first.id, TerminalReason::Completed)
            .is_none());
        assert_eq!(coordinator.current(), Some(current));
    }

    #[test]
    fn confirmation_belongs_to_the_active_operation() {
        let coordinator = OperationCoordinator::new();
        let operation = coordinator.start(OperationSource::WakeWord).unwrap();

        assert!(coordinator.confirm(operation.id).unwrap().confirmed);
        assert!(coordinator.confirm(operation.id + 1).is_none());
    }

    fn advance_to(coordinator: &OperationCoordinator, operation_id: u64, phase: OperationPhase) {
        match phase {
            OperationPhase::Recording => {}
            OperationPhase::Transcribing => {
                assert!(coordinator
                    .transition(operation_id, OperationPhase::Transcribing)
                    .is_some());
            }
            OperationPhase::Processing => {
                advance_to(coordinator, operation_id, OperationPhase::Transcribing);
                assert!(coordinator
                    .transition(operation_id, OperationPhase::Processing)
                    .is_some());
            }
            OperationPhase::Injecting => {
                advance_to(coordinator, operation_id, OperationPhase::Transcribing);
                assert!(coordinator
                    .transition(operation_id, OperationPhase::Injecting)
                    .is_some());
            }
        }
    }
}
