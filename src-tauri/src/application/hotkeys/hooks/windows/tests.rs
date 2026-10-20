//! Fake selected-button input only. These tests never install a global hook.
use super::*;

fn callback(trigger: DictationTrigger) -> Callback {
    Callback {
        queue: Arc::new(Queue::new(64)),
        trigger,
        down: false,
        primed: true,
        suppressed: false,
    }
}

#[test]
fn keyboard_filters_other_keys_and_synthetic_insertion_without_retaining_them() {
    for trigger in [
        DictationTrigger::RightAlt,
        DictationTrigger::RightCtrl,
        DictationTrigger::CapsLock,
    ] {
        let mut input = callback(trigger);
        for vk in 0..=255 {
            if vk == virtual_key(trigger) as u32 {
                continue;
            }
            assert!(!input.keyboard(vk, false, true, 1));
        }
        assert!(input.queue.edges.is_empty());
        assert!(!input.keyboard(virtual_key(trigger) as u32, true, true, 2));
        assert!(input.queue.edges.is_empty());
        input.keyboard(virtual_key(trigger) as u32, false, true, 3);
        assert!(input.queue.edges.pop().unwrap().pressed);
    }
}

#[test]
fn selected_caps_lock_swallows_repeat_and_release_but_only_queues_two_edges() {
    let mut input = callback(DictationTrigger::CapsLock);
    assert!(input.edge(true, 1));
    for time in 2..1000 {
        assert!(input.edge(true, time));
    }
    assert!(input.edge(false, 1000));
    assert_eq!(input.queue.edges.len(), 2);
    assert!(input.queue.edges.pop().unwrap().pressed);
    assert!(!input.queue.edges.pop().unwrap().pressed);
}

#[test]
fn caps_lock_overflow_keeps_a_suppressed_pair_then_passes_through() {
    let mut input = callback(DictationTrigger::CapsLock);
    input.queue = Arc::new(Queue::new(1));
    assert!(input.edge(true, 1));
    assert!(input.edge(false, 2)); // Overflow: no sticky suppression after this pair.
    assert!(input.queue.fault.load(Ordering::Acquire));
    assert!(!input.edge(true, 3));
    assert!(!input.edge(false, 4));

    let mut input = callback(DictationTrigger::CapsLock);
    assert!(input.edge(true, 1));
    input.queue.fail(); // A failure while held must still swallow that release.
    assert!(input.edge(false, 2));
    assert!(!input.edge(true, 3));
}

#[test]
fn activation_while_held_ignores_old_release_and_waits_for_fresh_press() {
    let mut input = callback(DictationTrigger::RightCtrl);
    input.down = true;
    input.primed = false;
    input.edge(true, 1);
    input.edge(false, 2);
    assert!(input.queue.edges.is_empty());
    input.edge(true, 3);
    assert_eq!(input.queue.edges.len(), 1);
}

#[test]
fn mouse_filters_other_button_and_injected_events_and_consumes_selected_pair() {
    for (trigger, selected, other) in [
        (DictationTrigger::XButton1, 1, 2),
        (DictationTrigger::XButton2, 2, 1),
    ] {
        let mut input = callback(trigger);
        assert!(!input.mouse(other, false, true, 1));
        assert!(!input.mouse(selected, true, true, 2));
        assert!(input.queue.edges.is_empty());
        assert!(input.mouse(selected, false, true, 3));
        assert!(input.mouse(selected, false, false, 4));
        assert_eq!(input.queue.edges.len(), 2);
    }
}
