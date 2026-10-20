use super::*;
use crate::application::hotkeys::state::{Action, ActiveCapture, ShortcutState};

fn edge(pressed: bool, time: u32) -> Edge {
    Edge { pressed, time }
}

#[test]
fn hold_and_mouse_toggle_forward_only_real_edges() {
    for trigger in [
        DictationTrigger::RightAlt,
        DictationTrigger::RightCtrl,
        DictationTrigger::CapsLock,
        DictationTrigger::XButton1,
        DictationTrigger::XButton2,
    ] {
        for mode in [HotkeyMode::Hold, HotkeyMode::Toggle] {
            if trigger.is_keyboard() && mode == HotkeyMode::Toggle {
                continue;
            }
            let mut gesture = GestureState::new(trigger, mode);
            assert_eq!(gesture.event(edge(false, 0)), Gesture::None);
            assert_eq!(
                gesture.event(edge(true, 1)),
                Gesture::Edge(KeyEvent::Pressed)
            );
            for time in 2..20 {
                assert_eq!(gesture.event(edge(true, time)), Gesture::None);
            }
            assert_eq!(
                gesture.event(edge(false, 20)),
                Gesture::Edge(KeyEvent::Released)
            );
            assert_eq!(gesture.event(edge(false, 21)), Gesture::None);
        }
    }
}

#[test]
fn modifier_toggle_requires_two_taps_and_does_not_reuse_second_tap() {
    for trigger in [
        DictationTrigger::RightAlt,
        DictationTrigger::RightCtrl,
        DictationTrigger::CapsLock,
    ] {
        let mut gesture = GestureState::new(trigger, HotkeyMode::Toggle);
        assert_eq!(gesture.event(edge(true, 0)), Gesture::None);
        assert_eq!(gesture.event(edge(true, 40)), Gesture::None);
        assert_eq!(gesture.event(edge(false, 50)), Gesture::None);
        assert_eq!(gesture.event(edge(true, 100)), Gesture::Toggle);
        assert_eq!(gesture.event(edge(true, 101)), Gesture::None);
        assert_eq!(gesture.event(edge(false, 150)), Gesture::None);
        assert_eq!(gesture.event(edge(true, 200)), Gesture::None);
        assert_eq!(gesture.event(edge(false, 220)), Gesture::None);
        assert_eq!(gesture.event(edge(true, 260)), Gesture::Toggle);
    }
}

#[test]
fn slow_and_held_first_taps_do_not_trigger_toggle() {
    let mut gesture = GestureState::new(DictationTrigger::RightAlt, HotkeyMode::Toggle);
    for (pressed, time) in [
        (true, 0),
        (false, 351),
        (true, 360),
        (false, 380),
        (true, 731),
    ] {
        assert_eq!(gesture.event(edge(pressed, time)), Gesture::None);
    }
    assert_eq!(gesture.event(edge(false, 750)), Gesture::None);
    assert_eq!(gesture.event(edge(true, 1100)), Gesture::Toggle); // Inclusive boundary.
}

#[test]
fn windows_tick_wrap_does_not_break_double_press() {
    let mut gesture = GestureState::new(DictationTrigger::CapsLock, HotkeyMode::Toggle);
    gesture.event(edge(true, u32::MAX - 30));
    gesture.event(edge(false, u32::MAX - 10));
    assert_eq!(gesture.event(edge(true, 20)), Gesture::Toggle);
}

#[test]
fn normalized_hold_release_cannot_stop_replacement_recording() {
    let mut gesture = GestureState::new(DictationTrigger::XButton1, HotkeyMode::Hold);
    let mut shortcut = ShortcutState::default();
    assert_eq!(
        gesture.event(edge(true, 1)),
        Gesture::Edge(KeyEvent::Pressed)
    );
    assert_eq!(
        shortcut.event(KeyEvent::Pressed, HotkeyMode::Hold, None),
        Action::Start
    );
    shortcut.started(10);
    // Overlay cancelled 10 and a different input started 11 while the button was held.
    assert_eq!(gesture.event(edge(true, 2)), Gesture::None);
    assert_eq!(
        gesture.event(edge(false, 3)),
        Gesture::Edge(KeyEvent::Released)
    );
    assert_eq!(
        shortcut.event(
            KeyEvent::Released,
            HotkeyMode::Hold,
            Some(ActiveCapture {
                operation: 11,
                recording: true
            })
        ),
        Action::None
    );
}
