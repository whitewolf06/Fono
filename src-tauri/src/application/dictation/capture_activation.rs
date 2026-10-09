//! Activate only the surviving capture; cleanup may never resume over its successor.
pub(super) fn activate(
    pause_if_active: impl FnOnce() -> bool,
    enter_listening_if_active: impl FnOnce() -> bool,
    resume_if_idle: impl FnOnce(),
) -> bool {
    if pause_if_active() && enter_listening_if_active() {
        return true;
    }
    resume_if_idle();
    false
}

#[cfg(test)]
mod tests {
    use super::activate;
    use fono_core::{OperationCoordinator, OperationPhase, OperationSource};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn cancellation_during_target_probe_never_pauses_resumed_wake() {
        let operations = OperationCoordinator::new();
        let operation = operations.start(OperationSource::Hotkey).unwrap();
        let paused = AtomicBool::new(false);
        operations.cancel(operation.id).unwrap();

        assert!(!activate(
            || {
                if !operations.is_active(operation.id) {
                    return false;
                }
                paused.store(true, Ordering::SeqCst);
                true
            },
            || panic!("a cancelled start must not publish Listening"),
            || {
                if operations.current().is_none() {
                    paused.store(false, Ordering::SeqCst);
                }
            },
        ));
        assert!(!paused.load(Ordering::SeqCst));
        assert!(operations.current().is_none());
    }

    #[test]
    fn cancellation_between_pause_and_listening_restores_idle_wake() {
        let operations = OperationCoordinator::new();
        let operation = operations.start(OperationSource::Hotkey).unwrap();
        let paused = AtomicBool::new(false);

        assert!(!activate(
            || {
                assert!(operations.is_active(operation.id));
                paused.store(true, Ordering::SeqCst);
                true
            },
            || {
                operations.cancel(operation.id).unwrap();
                operations
                    .transition(operation.id, OperationPhase::Recording)
                    .is_some()
            },
            || {
                if operations.current().is_none() {
                    paused.store(false, Ordering::SeqCst);
                }
            },
        ));
        assert!(!paused.load(Ordering::SeqCst));
    }

    #[test]
    fn stale_activation_cleanup_does_not_resume_over_a_new_capture() {
        let operations = OperationCoordinator::new();
        let old = operations.start(OperationSource::Hotkey).unwrap();
        operations.cancel(old.id).unwrap();
        let next = operations.start(OperationSource::Ui).unwrap();
        let paused = AtomicBool::new(true);

        assert!(!activate(
            || operations.is_active(old.id),
            || panic!("an old start must not publish Listening over its successor"),
            || {
                if operations.current().is_none() {
                    paused.store(false, Ordering::SeqCst);
                }
            },
        ));
        assert!(paused.load(Ordering::SeqCst));
        assert_eq!(operations.current().unwrap().id, next.id);
    }
}
