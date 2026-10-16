//! Wait for physical modifier release; never synthesize releases of user keys.
use crate::error::{AppError, AppResult};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_millis(500);
const INTERVAL: Duration = Duration::from_millis(10);
const HELD: &str = "Отпустите Ctrl, Shift, Alt и Win. Текст сохранён в Fono для копирования";

pub(super) fn wait_for_release(cancelled: impl Fn() -> bool) -> AppResult<()> {
    let start = Instant::now();
    wait_with(held, cancelled, || start.elapsed(), std::thread::sleep)
}

pub(super) fn ensure_released() -> AppResult<()> {
    if held() {
        Err(AppError::Injection(HELD.into()))
    } else {
        Ok(())
    }
}

fn wait_with(
    mut held: impl FnMut() -> bool,
    cancelled: impl Fn() -> bool,
    elapsed: impl Fn() -> Duration,
    mut sleep: impl FnMut(Duration),
) -> AppResult<()> {
    loop {
        if cancelled() {
            return Err(AppError::Cancelled(
                "insertion was cancelled before modifier release".into(),
            ));
        }
        if !held() {
            return Ok(());
        }
        let remaining = TIMEOUT.saturating_sub(elapsed());
        if remaining.is_zero() {
            return Err(AppError::Injection(HELD.into()));
        }
        sleep(INTERVAL.min(remaining));
    }
}

#[cfg(windows)]
fn held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // The high bit is current key state. The low bit is shared across callers
    // and is deliberately ignored. Generic Ctrl/Shift/Alt cover both sides.
    [0x10, 0x11, 0x12, 0x5b, 0x5c]
        .into_iter()
        .any(|key| key_is_down(unsafe { GetAsyncKeyState(key) }))
}

#[cfg(not(windows))]
fn held() -> bool {
    false
}

fn key_is_down(state: i16) -> bool {
    state as u16 & 0x8000 != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn slow_ctrl_release_waits_before_permitting_any_input() {
        let elapsed = Cell::new(Duration::ZERO);
        let probes = Cell::new(0);
        let result = wait_with(
            || {
                probes.set(probes.get() + 1);
                probes.get() <= 7
            },
            || false,
            || elapsed.get(),
            |wait| elapsed.set(elapsed.get() + wait),
        );
        assert!(result.is_ok());
        assert_eq!(elapsed.get(), Duration::from_millis(70));
        assert_eq!(probes.get(), 8);
    }

    #[test]
    fn stuck_modifier_times_out_without_synthetic_release() {
        let elapsed = Cell::new(Duration::ZERO);
        let result = wait_with(
            || true,
            || false,
            || elapsed.get(),
            |wait| elapsed.set(elapsed.get() + wait),
        );
        assert!(result.is_err());
        assert_eq!(elapsed.get(), TIMEOUT);
    }

    #[test]
    fn cancellation_during_wait_never_authorizes_input() {
        let elapsed = Cell::new(Duration::ZERO);
        let result = wait_with(
            || true,
            || elapsed.get() >= Duration::from_millis(30),
            || elapsed.get(),
            |wait| elapsed.set(elapsed.get() + wait),
        );
        assert!(matches!(result, Err(AppError::Cancelled(_))));
        assert_eq!(elapsed.get(), Duration::from_millis(30));
    }

    #[test]
    fn current_high_bit_is_used_and_shared_pressed_bit_is_ignored() {
        assert!(!key_is_down(0));
        assert!(!key_is_down(1));
        assert!(key_is_down(i16::MIN));
        assert!(key_is_down(-32767));
    }
}
