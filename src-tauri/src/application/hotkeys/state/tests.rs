use super::*;

fn recording(operation: u64) -> Option<ActiveCapture> {
    Some(ActiveCapture {
        operation,
        recording: true,
    })
}

#[test]
fn hold_starts_on_press_and_stops_exact_capture_on_release() {
    let mut key = ShortcutState::default();
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Hold, None),
        Action::Start
    );
    key.started(11);
    assert_eq!(
        key.event(KeyEvent::Released, HotkeyMode::Hold, recording(11)),
        Action::Stop(11)
    );
    assert_eq!(
        key.event(KeyEvent::Released, HotkeyMode::Hold, recording(11)),
        Action::None
    );
}

#[test]
fn toggle_keeps_recording_after_release_and_stops_on_second_press() {
    let mut key = ShortcutState::default();
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, None),
        Action::Start
    );
    key.started(12);
    assert_eq!(
        key.event(KeyEvent::Released, HotkeyMode::Toggle, recording(12)),
        Action::None
    );
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, recording(12)),
        Action::Stop(12)
    );
    assert_eq!(
        key.event(KeyEvent::Released, HotkeyMode::Toggle, recording(12)),
        Action::None
    );
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, None),
        Action::Start
    );
}

#[test]
fn autorepeat_never_stops_or_starts_again() {
    for mode in [HotkeyMode::Hold, HotkeyMode::Toggle] {
        let mut key = ShortcutState::default();
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::Start);
        key.started(13);
        for _ in 0..5 {
            assert_eq!(
                key.event(KeyEvent::Pressed, mode, recording(13)),
                Action::None
            );
        }
    }
}

#[test]
fn failed_start_waits_for_release_before_retry() {
    for mode in [HotkeyMode::Hold, HotkeyMode::Toggle] {
        let mut key = ShortcutState::default();
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::Start);
        // The adapter did not call started because capture failed.
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::None);
        assert_eq!(key.event(KeyEvent::Released, mode, None), Action::None);
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::Start);
    }
}

#[test]
fn release_after_overlay_stop_does_not_stop_new_session() {
    let mut key = ShortcutState::default();
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Hold, None),
        Action::Start
    );
    key.started(14);
    assert_eq!(
        key.event(KeyEvent::Released, HotkeyMode::Hold, recording(15)),
        Action::None
    );
}

#[test]
fn toggle_can_restart_after_overlay_cancel_or_finished_processing() {
    for mode in [HotkeyMode::Hold, HotkeyMode::Toggle] {
        let mut key = ShortcutState::default();
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::Start);
        key.started(16);
        assert_eq!(key.event(KeyEvent::Released, mode, None), Action::None);
        assert_eq!(key.event(KeyEvent::Pressed, mode, None), Action::Start);
    }
}

#[test]
fn processing_and_unrelated_recording_do_not_start_or_stop() {
    let mut key = ShortcutState::default();
    key.started(17);
    for capture in [
        recording(18),
        Some(ActiveCapture {
            operation: 17,
            recording: false,
        }),
    ] {
        assert_eq!(
            key.event(KeyEvent::Pressed, HotkeyMode::Toggle, capture),
            Action::None
        );
        assert_eq!(
            key.event(KeyEvent::Released, HotkeyMode::Toggle, capture),
            Action::None
        );
    }
}

#[test]
fn failed_toggle_flush_restores_stop_for_the_same_capture() {
    let mut key = ShortcutState::default();
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, None),
        Action::Start
    );
    key.started(42);
    key.event(KeyEvent::Released, HotkeyMode::Toggle, recording(42));
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, recording(42)),
        Action::Stop(42)
    );
    key.restore_stop(42, recording(42));
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, recording(42)),
        Action::None
    );
    key.event(KeyEvent::Released, HotkeyMode::Toggle, recording(42));
    assert_eq!(
        key.event(KeyEvent::Pressed, HotkeyMode::Toggle, recording(42)),
        Action::Stop(42)
    );
}

#[test]
fn late_flush_failure_cannot_restore_a_replaced_or_completed_capture() {
    let mut key = ShortcutState::default();
    key.restore_stop(42, recording(43));
    assert_eq!(key.operation, None);
    key.restore_stop(
        42,
        Some(ActiveCapture {
            operation: 42,
            recording: false,
        }),
    );
    assert_eq!(key.operation, None);
    key.started(43);
    key.restore_stop(42, recording(42));
    assert_eq!(key.operation, Some(43));
}
