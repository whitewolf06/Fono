//! Availability belongs to one capture, never to the process-wide STT route.
use super::Pipeline;
use fono_core::{OperationCancellation, OperationEvent, OperationPhase, TerminalReason};
use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteCheckSnapshot {
    pub operation_id: u64,
    #[serde(flatten)]
    pub state: RemoteCheckState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RemoteCheckState {
    Checking,
    Ready,
    Starting,
    Stopping,
    Busy,
    ModelNotReady,
    Unavailable { reason: RemoteCheckReason },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCheckReason {
    Network,
    Timeout,
    Unauthorized,
    NotConfigured,
    IncompatibleVersion,
    UnsupportedCapabilities,
    InvalidResponse,
}

pub(super) struct RemoteCheck {
    snapshot: RemoteCheckSnapshot,
    cancellation: OperationCancellation,
}

impl Pipeline {
    /// Allocate an independent cancellation view before queuing network work.
    /// Repeated setup for the same capture cannot start another probe.
    #[cfg(test)]
    pub(crate) fn begin_remote_check_for(&self, operation: u64) -> Option<OperationCancellation> {
        self.begin_remote_check_and_publish_for(operation, |_| {})
    }

    pub(crate) fn begin_remote_check_and_publish_for(
        &self,
        operation: u64,
        publish: impl FnOnce(&RemoteCheckSnapshot),
    ) -> Option<OperationCancellation> {
        let _capture = self.recording.lock();
        if !self.remote_check_editable(operation) {
            return None;
        }
        let mut check = self.remote_check.lock();
        if check
            .as_ref()
            .is_some_and(|check| check.snapshot.operation_id == operation)
        {
            return None;
        }
        if let Some(previous) = check.take() {
            previous.cancellation.cancel();
        }
        let cancellation =
            OperationCancellation::default().combined_with(&self.cancellation(operation)?);
        *check = Some(RemoteCheck {
            snapshot: RemoteCheckSnapshot {
                operation_id: operation,
                state: RemoteCheckState::Checking,
            },
            cancellation: cancellation.clone(),
        });
        publish(&check.as_ref().expect("new availability slot").snapshot);
        Some(cancellation)
    }

    #[cfg(test)]
    pub(crate) fn complete_remote_check_for(
        &self,
        operation: u64,
        state: RemoteCheckState,
    ) -> bool {
        self.complete_remote_check_and_publish_for(operation, state, |_| {})
    }

    /// Publication shares capture and terminal ownership fences. The callback
    /// must not reacquire pipeline locks or perform blocking network/device I/O.
    pub(crate) fn complete_remote_check_and_publish_for(
        &self,
        operation: u64,
        state: RemoteCheckState,
        publish: impl FnOnce(&RemoteCheckSnapshot),
    ) -> bool {
        let _capture = self.recording.lock();
        if !self.remote_check_editable(operation)
            || matches!(
                state,
                RemoteCheckState::Checking | RemoteCheckState::Cancelled
            )
        {
            return false;
        }
        let mut check = self.remote_check.lock();
        let Some(check) = check.as_mut().filter(|check| {
            check.snapshot.operation_id == operation
                && check.snapshot.state == RemoteCheckState::Checking
                && !check.cancellation.is_cancelled()
        }) else {
            return false;
        };
        check.snapshot.state = state;
        publish(&check.snapshot);
        true
    }

    pub(crate) fn remote_check_for(&self, operation: u64) -> Option<RemoteCheckSnapshot> {
        let check = self.remote_check.lock();
        check
            .as_ref()
            .filter(|check| {
                check.snapshot.operation_id == operation && self.is_operation_active(operation)
            })
            .map(|check| check.snapshot.clone())
    }

    /// Stop cancels only a pending preflight. Already observed availability is
    /// retained for the frozen session and future explicit fallback policy.
    #[cfg(test)]
    pub(crate) fn cancel_remote_check_for(&self, operation: u64) -> bool {
        let _capture = self.recording.lock();
        self.cancel_remote_check_locked(operation)
    }

    pub(super) fn cancel_remote_check_locked(&self, operation: u64) -> bool {
        let mut check = self.remote_check.lock();
        let Some(check) = check.as_mut().filter(|check| {
            check.snapshot.operation_id == operation
                && check.snapshot.state == RemoteCheckState::Checking
        }) else {
            return false;
        };
        check.cancellation.cancel();
        check.snapshot.state = RemoteCheckState::Cancelled;
        true
    }

    pub(super) fn retire_remote_check_for(&self, operation: u64) {
        let mut check = self.remote_check.lock();
        if check
            .as_ref()
            .is_some_and(|check| check.snapshot.operation_id == operation)
        {
            if let Some(check) = check.take() {
                check.cancellation.cancel();
            }
        }
    }

    pub(super) fn reset_remote_check_for_new(&self, operation: u64) {
        let mut check = self.remote_check.lock();
        if check
            .as_ref()
            .is_some_and(|check| check.snapshot.operation_id != operation)
        {
            if let Some(check) = check.take() {
                check.cancellation.cancel();
            }
        }
    }

    /// Some finish callers already own the capture lock. Use the availability
    /// slot to serialize terminal retirement against its publication instead
    /// of recursively taking that non-reentrant capture lock.
    pub(super) fn finish_operation_with_remote_check(
        &self,
        operation: u64,
        reason: TerminalReason,
    ) -> Option<OperationEvent> {
        let mut check = self.remote_check.lock();
        let event = self.operations.finish(operation, reason)?;
        if check
            .as_ref()
            .is_some_and(|check| check.snapshot.operation_id == operation)
        {
            if let Some(check) = check.take() {
                check.cancellation.cancel();
            }
        }
        Some(event)
    }

    fn remote_check_editable(&self, operation: u64) -> bool {
        operation != 0
            && self.processing_frozen_for.load(Ordering::Acquire) != operation
            && self.current_operation().is_some_and(|current| {
                current.id == operation && current.phase == OperationPhase::Recording
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_status_is_flat_and_contains_no_server_configuration() {
        let value = serde_json::to_value(RemoteCheckSnapshot {
            operation_id: 12,
            state: RemoteCheckState::Unavailable {
                reason: RemoteCheckReason::Timeout,
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "operation_id": 12, "state": "unavailable", "reason": "timeout"
            })
        );
    }
}
