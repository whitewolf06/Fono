//! Renderer callbacks keep their original operation across the IPC queue.
use super::Pipeline;
use crate::error::{AppError, AppResult};

impl Pipeline {
    /// A terminal close must not act as cancellation of a successor operation.
    /// Capture admission shares this fence, so an accepted hide completes first.
    pub(crate) fn dismiss_overlay_when_idle(
        &self,
        has_pending: impl FnOnce() -> bool,
        dismiss: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<()> {
        let capture = self.recording.lock();
        if *capture
            || self
                .capture_operation
                .load(std::sync::atomic::Ordering::Acquire)
                != 0
            || self.current_operation().is_some()
            || has_pending()
        {
            return Err(AppError::Busy(
                "Диктовка ещё выполняется. Закрытие старого индикатора отменено".into(),
            ));
        }
        dismiss()
    }

    /// Missing identity preserves legacy clients; new overlays send an exact ID.
    pub(crate) fn requested_operation(&self, expected: Option<u64>) -> AppResult<u64> {
        match expected {
            Some(id) if self.is_operation_active(id) => Ok(id),
            Some(_) => Err(AppError::Cancelled(
                "Эта диктовка уже завершена или заменена".into(),
            )),
            None => Ok(self.operation_id()),
        }
    }

    pub(crate) fn confirm_for(&self, operation: u64) -> bool {
        let _capture = self.recording.lock();
        if self.operations.confirm(operation).is_none() {
            return false;
        }
        tracing::debug!(operation, "pipeline: confirmation requested");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fono_core::OperationSource;

    #[test]
    fn queued_old_stop_cancel_and_confirm_cannot_select_the_successor() {
        let pipeline = Pipeline::new();
        let old = pipeline.operations.start(OperationSource::Hotkey).unwrap();
        pipeline.cancel_for(old.id).unwrap();
        let next = pipeline
            .operations
            .start(OperationSource::WakeWord)
            .unwrap();

        assert!(pipeline.requested_operation(Some(old.id)).is_err());
        assert!(!pipeline.confirm_for(old.id));
        assert!(pipeline.cancel_for(old.id).is_none());
        let current = pipeline.current_operation().unwrap();
        assert_eq!(current.id, next.id);
        assert!(!current.confirmed);
        assert_eq!(
            pipeline.requested_operation(Some(next.id)).unwrap(),
            next.id
        );
        assert_eq!(pipeline.requested_operation(None).unwrap(), next.id);
    }

    #[test]
    fn confirmation_race_after_validation_is_bound_to_the_same_id() {
        let pipeline = Pipeline::new();
        let first = pipeline
            .operations
            .start(OperationSource::WakeWord)
            .unwrap();
        let captured = pipeline.requested_operation(Some(first.id)).unwrap();
        pipeline.cancel_for(first.id).unwrap();
        let next = pipeline.operations.start(OperationSource::Ui).unwrap();
        assert!(!pipeline.confirm_for(captured));
        assert!(!pipeline.current_operation().unwrap().confirmed);
        assert!(pipeline.confirm_for(next.id));
        assert!(pipeline.current_operation().unwrap().confirmed);
    }

    #[test]
    fn idle_explicit_ids_are_stale_while_legacy_noop_identity_remains_zero() {
        let pipeline = Pipeline::new();
        assert!(pipeline.requested_operation(Some(0)).is_err());
        assert!(pipeline.requested_operation(Some(15)).is_err());
        assert_eq!(pipeline.requested_operation(None).unwrap(), 0);
        assert!(!pipeline.confirm_for(0));
    }
}

#[cfg(test)]
#[path = "session_control_dismissal_tests.rs"]
mod dismissal_tests;
