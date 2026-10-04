//! Interactive capture is independent of STT. Background windows yield and
//! restart at their checkpoint when an interactive session reserves the engine.
use crate::{
    error::{AppError, AppResult},
    operation::OperationCancellation,
};
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
pub struct SttScheduler {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    interactive: Option<u64>,
    occupied: bool,
    background_cancel: Option<OperationCancellation>,
}

pub struct SttPermit {
    scheduler: Arc<SttScheduler>,
    pub cancellation: OperationCancellation,
}

impl SttScheduler {
    pub(crate) fn is_busy(&self) -> bool {
        let state = self.state.lock();
        state.occupied || state.interactive.is_some()
    }

    pub fn reserve(&self, operation: u64) {
        let mut state = self.state.lock();
        state.interactive = Some(operation);
        if let Some(cancel) = &state.background_cancel {
            cancel.cancel();
        }
        self.changed.notify_all();
    }

    pub fn release_reservation(&self, operation: u64) {
        let mut state = self.state.lock();
        if state.interactive == Some(operation) {
            state.interactive = None;
        }
        self.changed.notify_all();
    }

    pub fn acquire(
        self: &Arc<Self>,
        interactive: bool,
        cancel: &OperationCancellation,
    ) -> AppResult<SttPermit> {
        let mut state = self.state.lock();
        loop {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled(
                    "Ожидание распознавания отменено".into(),
                ));
            }
            if !state.occupied && (interactive || state.interactive.is_none()) {
                break;
            }
            self.changed.wait_for(&mut state, Duration::from_millis(20));
        }
        state.occupied = true;
        let cancellation = OperationCancellation::default().combined_with(cancel);
        if !interactive {
            state.background_cancel = Some(cancellation.clone());
        }
        Ok(SttPermit {
            scheduler: self.clone(),
            cancellation,
        })
    }
}

impl Drop for SttPermit {
    fn drop(&mut self) {
        let mut state = self.scheduler.state.lock();
        state.occupied = false;
        state.background_cancel = None;
        self.scheduler.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_preempts_only_the_background_window() {
        let scheduler = Arc::new(SttScheduler::default());
        let job = OperationCancellation::default();
        let permit = scheduler.acquire(false, &job).unwrap();
        scheduler.reserve(1);
        assert!(permit.cancellation.is_cancelled());
        assert!(!job.is_cancelled());
        drop(permit);
        let interactive = scheduler.acquire(true, &job).unwrap();
        assert!(!interactive.cancellation.is_cancelled());
    }
    #[test]
    fn cancellation_interrupts_wait_and_old_release_keeps_new_reservation() {
        let scheduler = Arc::new(SttScheduler::default());
        scheduler.reserve(2);
        scheduler.release_reservation(1);
        let cancel = OperationCancellation::default();
        cancel.cancel();
        assert!(scheduler.acquire(false, &cancel).is_err());
        assert_eq!(scheduler.state.lock().interactive, Some(2));
    }
}
