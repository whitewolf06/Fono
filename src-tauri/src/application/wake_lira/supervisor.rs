//! A transient detector failure must not silently turn an enabled listener off.
use crate::error::{AppError, AppResult};

const MAX_RESTARTS: u8 = 2;

pub(super) fn run(
    mut attempt: impl FnMut() -> AppResult<()>,
    cancelled: impl Fn() -> bool,
    mut recover: impl FnMut(u8, &AppError) -> bool,
) -> AppResult<()> {
    let mut restarts = 0;
    loop {
        if cancelled() {
            return Ok(());
        }
        match attempt() {
            Err(error)
                if !cancelled()
                    && restarts < MAX_RESTARTS
                    && matches!(error, AppError::Internal(_) | AppError::Audio(_)) =>
            {
                restarts += 1;
                // Audio is released before retry. The caller keeps healthy
                // models on an audio failure, and replaces only a failed worker.
                if !recover(restarts, &error) {
                    return Ok(());
                }
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn failed_listener_can_wake_again_without_another_enable() {
        let attempts = Cell::new(0);
        let wakes = Cell::new(0);
        let recoveries = Cell::new(0);
        run(
            || {
                attempts.set(attempts.get() + 1);
                wakes.set(wakes.get() + 1);
                if attempts.get() == 1 {
                    Err(AppError::Internal("worker reply timed out".into()))
                } else {
                    Ok(())
                }
            },
            || false,
            |restart, _| {
                recoveries.set(restart);
                true
            },
        )
        .unwrap();
        assert_eq!(wakes.get(), 2);
        assert_eq!(recoveries.get(), 1);
    }

    #[test]
    fn persistent_failure_has_a_terminal_error_instead_of_an_endless_retry() {
        let attempts = Cell::new(0);
        let result = run(
            || {
                attempts.set(attempts.get() + 1);
                Err(AppError::Audio("input interrupted".into()))
            },
            || false,
            |_, _| true,
        );
        assert!(matches!(result, Err(AppError::Audio(_))));
        assert_eq!(attempts.get(), 3);
    }

    #[test]
    fn stop_during_recovery_never_reopens_microphone_or_worker() {
        let stopped = Cell::new(false);
        let attempts = Cell::new(0);
        run(
            || {
                attempts.set(attempts.get() + 1);
                Err(AppError::Internal("pipe closed".into()))
            },
            || stopped.get(),
            |_, _| {
                stopped.set(true);
                true
            },
        )
        .unwrap();
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn configuration_and_cancelled_operations_are_not_retried() {
        for error in [
            AppError::Config("missing model".into()),
            AppError::Cancelled("explicit stop".into()),
        ] {
            let mut error = Some(error);
            assert!(run(
                || Err(error.take().expect("no retry")),
                || false,
                |_, _| panic!("this failure is terminal"),
            )
            .is_err());
        }
    }
}
