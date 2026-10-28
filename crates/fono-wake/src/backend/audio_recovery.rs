use crate::WakeWordError;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const DELAYS_MS: [u64; 3] = [500, 2_000, 5_000];
#[derive(Default)]
pub(super) struct AudioRecovery {
    retries: usize,
}
impl AudioRecovery {
    pub(super) fn next(&mut self, error: &WakeWordError, stable_for: Duration) -> Option<Duration> {
        if !recoverable(error) {
            return None;
        }
        if stable_for >= Duration::from_secs(10) {
            self.retries = 0;
        }
        let delay = DELAYS_MS.get(self.retries).copied()?;
        self.retries += 1;
        Some(Duration::from_millis(delay))
    }
    pub(super) fn attempt(&self) -> usize {
        self.retries
    }
}
fn recoverable(error: &WakeWordError) -> bool {
    match error {
        WakeWordError::Audio(message) => ![
            "zero sample rate",
            "subscriber limit",
            "different audio inputs",
            "owner is unavailable",
            "handoff queue",
            "cursor",
            "input changed",
        ]
        .iter()
        .any(|issue| message.contains(issue)),
        _ => false,
    }
}
pub(super) fn wait_while_running(running: &AtomicBool, duration: Duration) -> bool {
    let until = Instant::now() + duration;
    while running.load(Ordering::Acquire) {
        let now = Instant::now();
        if now >= until {
            return true;
        }
        std::thread::sleep((until - now).min(Duration::from_millis(10)));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_disconnects_get_exactly_three_retries_and_bad_models_get_none() {
        let mut recovery = AudioRecovery::default();
        let unplugged = WakeWordError::Audio("device is disconnected".into());
        let delays: Vec<_> = (0..4)
            .map(|_| recovery.next(&unplugged, Duration::ZERO))
            .collect();
        assert_eq!(
            delays,
            vec![
                Some(Duration::from_millis(500)),
                Some(Duration::from_secs(2)),
                Some(Duration::from_secs(5)),
                None
            ]
        );
        assert!(AudioRecovery::default()
            .next(
                &WakeWordError::ModelLoad("bad model".into()),
                Duration::ZERO
            )
            .is_none());
        assert!(AudioRecovery::default()
            .next(
                &WakeWordError::Audio("consumers requested different audio inputs".into()),
                Duration::ZERO
            )
            .is_none());
    }
    #[test]
    fn stop_interrupts_a_five_second_backoff_promptly() {
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let started = Instant::now();
        let worker =
            std::thread::spawn(move || wait_while_running(&worker_running, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(15));
        running.store(false, Ordering::Release);
        assert!(!worker.join().unwrap());
        assert!(started.elapsed() < Duration::from_millis(500));
    }
    #[test]
    fn a_stable_new_capture_restores_the_recovery_budget() {
        let mut recovery = AudioRecovery::default();
        let error = WakeWordError::Audio("device suspended".into());
        for _ in 0..3 {
            assert!(recovery.next(&error, Duration::ZERO).is_some());
        }
        assert_eq!(
            recovery.next(&error, Duration::from_secs(10)),
            Some(Duration::from_millis(500))
        );
    }
}
