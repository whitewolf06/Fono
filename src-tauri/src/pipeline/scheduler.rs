//! Interactive capture is independent of STT. Background windows yield and
//! restart at their checkpoint when an interactive session reserves the engine.
use crate::error::{AppError, AppResult};
use fono_core::OperationCancellation;
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
    stopping: Option<u64>,
    interactive_waiters: usize,
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
        state.stopping = None;
        if let Some(cancel) = &state.background_cancel {
            cancel.cancel();
        }
        self.changed.notify_all();
    }

    pub fn release_reservation(&self, operation: u64) {
        let mut state = self.state.lock();
        if state.interactive == Some(operation) {
            state.interactive = None;
            state.stopping = None;
        }
        self.changed.notify_all();
    }

    pub fn acquire(
        self: &Arc<Self>,
        interactive: bool,
        cancel: &OperationCancellation,
    ) -> AppResult<SttPermit> {
        self.acquire_request(
            if interactive {
                Request::Interactive
            } else {
                Request::Service
            },
            cancel,
        )
    }

    /// Only this capture may use its reservation; Stop permanently closes
    /// this lane while preserving the ordinary interactive reservation.
    pub fn acquire_capture(
        self: &Arc<Self>,
        operation: u64,
        cancel: &OperationCancellation,
    ) -> AppResult<SttPermit> {
        self.acquire_request(Request::Capture(operation), cancel)
    }

    pub fn promote_stop(&self, operation: u64) {
        let mut state = self.state.lock();
        if state.interactive != Some(operation) {
            return;
        }
        state.stopping = Some(operation);
        if let Some(cancel) = &state.background_cancel {
            cancel.cancel();
        }
        self.changed.notify_all();
    }

    fn acquire_request(
        self: &Arc<Self>,
        request: Request,
        cancel: &OperationCancellation,
    ) -> AppResult<SttPermit> {
        let mut state = self.state.lock();
        let interactive = matches!(request, Request::Interactive);
        if interactive && !cancel.is_cancelled() {
            state.interactive_waiters += 1;
            if let Some(background) = &state.background_cancel {
                background.cancel();
            }
        } else if cancel.is_cancelled() {
            return Err(AppError::Cancelled("STT request cancelled".into()));
        }
        loop {
            let retired_capture = matches!(request, Request::Capture(id)
                if state.interactive != Some(id) || state.stopping == Some(id));
            if cancel.is_cancelled() || retired_capture {
                if interactive {
                    state.interactive_waiters -= 1;
                    self.changed.notify_all();
                }
                return Err(AppError::Cancelled(
                    "Ожидание распознавания отменено".into(),
                ));
            }
            let allowed = match request {
                Request::Interactive => true,
                Request::Service => state.interactive.is_none() && state.interactive_waiters == 0,
                Request::Capture(_) => state.interactive_waiters == 0,
            };
            if !state.occupied && allowed {
                break;
            }
            self.changed.wait_for(&mut state, Duration::from_millis(20));
        }
        if interactive {
            state.interactive_waiters -= 1;
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

#[derive(Clone, Copy)]
enum Request {
    Interactive,
    Service,
    Capture(u64),
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

    #[test]
    fn capture_lane_is_owned_and_a_stale_stop_cannot_preempt_its_replacement() {
        let scheduler = Arc::new(SttScheduler::default());
        let cancellation = OperationCancellation::default();
        scheduler.reserve(1);
        let old = scheduler.acquire_capture(1, &cancellation).unwrap();
        scheduler.reserve(2);
        assert!(old.cancellation.is_cancelled());
        drop(old);
        assert!(scheduler.acquire_capture(1, &cancellation).is_err());
        let current = scheduler.acquire_capture(2, &cancellation).unwrap();
        scheduler.promote_stop(1);
        assert!(!current.cancellation.is_cancelled());
        scheduler.promote_stop(2);
        assert!(current.cancellation.is_cancelled());
        assert!(!cancellation.is_cancelled());
        drop(current);
        assert!(scheduler.acquire_capture(2, &cancellation).is_err());
        let _stop = scheduler.acquire(true, &cancellation).unwrap();
    }

    #[test]
    fn cancelled_interactive_waiter_does_not_leave_the_capture_lane_blocked() {
        let scheduler = Arc::new(SttScheduler::default());
        let capture_cancel = OperationCancellation::default();
        scheduler.reserve(1);
        let capture = scheduler.acquire_capture(1, &capture_cancel).unwrap();
        let interactive_cancel = OperationCancellation::default();
        let pending = interactive_cancel.clone();
        let worker_scheduler = scheduler.clone();
        let waiter = std::thread::spawn(move || worker_scheduler.acquire(true, &pending));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while scheduler.state.lock().interactive_waiters != 1 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(capture.cancellation.is_cancelled());
        assert!(!capture_cancel.is_cancelled());
        interactive_cancel.cancel();
        assert!(waiter.join().unwrap().is_err());
        assert_eq!(scheduler.state.lock().interactive_waiters, 0);
        drop(capture);
        let _next = scheduler.acquire_capture(1, &capture_cancel).unwrap();
    }
}
