use super::*;
use std::cell::RefCell;

#[test]
fn duplicate_notifications_share_one_probe_but_windows_are_independent() {
    let mut policy = Policy::default();
    let main = policy.attach(Surface::Settings);
    let overlay = policy.attach(Surface::Overlay);
    let first = policy.request(Surface::Settings, main).unwrap();
    assert!(policy.request(Surface::Settings, main).is_none());
    assert!(policy.request(Surface::Overlay, overlay).is_some());
    policy.finish(first);
    assert!(policy.request(Surface::Settings, main).is_some());
}

#[test]
fn late_completion_does_not_clear_a_new_probe() {
    let mut policy = Policy::default();
    let main = policy.attach(Surface::Settings);
    let first = policy.request(Surface::Settings, main).unwrap();
    policy.finish(first);
    let second = policy.request(Surface::Settings, main).unwrap();
    policy.finish(first);
    assert!(policy.current(second));
    assert!(!policy.current(first));
}

#[test]
fn replacing_a_window_invalidates_old_hooks_and_timeouts() {
    let mut policy = Policy::default();
    let old = policy.attach(Surface::Overlay);
    let ticket = policy.request(Surface::Overlay, old).unwrap();
    let new = policy.attach(Surface::Overlay);
    policy.detach(Surface::Overlay, old);
    assert!(!policy.current(ticket));
    assert!(policy.request(Surface::Overlay, old).is_none());
    assert!(policy.request(Surface::Overlay, new).is_some());
}

#[test]
fn destroyed_windows_cannot_be_repaired_by_a_late_callback() {
    let mut policy = Policy::default();
    let instance = policy.attach(Surface::Settings);
    let ticket = policy.request(Surface::Settings, instance).unwrap();
    policy.detach(Surface::Settings, instance);
    assert!(!policy.current(ticket));
    assert!(policy.request(Surface::Settings, instance).is_none());
}

#[test]
fn shutdown_cancels_pending_and_future_work() {
    let mut policy = Policy::default();
    let instance = policy.attach(Surface::Settings);
    let ticket = policy.request(Surface::Settings, instance).unwrap();
    policy.close();
    assert!(policy.is_closed());
    assert!(!policy.current(ticket));
    assert!(!policy.restore_allowed(ticket));
    assert!(policy.request(Surface::Settings, instance).is_none());
    let new = policy.attach(Surface::Settings);
    assert!(policy.request(Surface::Settings, new).is_none());
}

#[test]
fn nested_restore_keeps_process_alive_until_both_creations_finish() {
    let flight = InFlight::default();
    assert!(!flight.active());
    let outer = flight.start();
    {
        let _nested = flight.start();
        assert!(flight.active());
    }
    assert!(flight.active());
    drop(outer);
    assert!(!flight.active());
}

#[test]
fn restore_waits_for_destroy_without_replacing_a_newer_window() {
    let mut policy = Policy::default();
    let instance = policy.attach(Surface::Settings);
    let ticket = policy.request(Surface::Settings, instance).unwrap();
    assert!(policy.restore_allowed(ticket));
    policy.detach(Surface::Settings, instance);
    assert!(policy.restore_allowed(ticket));
    policy.attach(Surface::Settings);
    assert!(!policy.restore_allowed(ticket));
}

#[test]
fn only_owned_window_labels_are_recoverable() {
    assert_eq!(Surface::from_label("settings").unwrap().label(), "settings");
    assert_eq!(Surface::from_label("overlay").unwrap().label(), "overlay");
    assert!(Surface::from_label("other").is_none());
}

struct FakeSurface {
    visible: bool,
    bounds_fail: bool,
    restore_fail: bool,
    calls: RefCell<Vec<&'static str>>,
}

impl RenderSurface for FakeSurface {
    type Error = &'static str;
    fn visible(&self) -> Result<bool, Self::Error> {
        Ok(self.visible)
    }
    fn set_visible(&self, visible: bool) -> Result<(), Self::Error> {
        self.calls
            .borrow_mut()
            .push(if visible { "restore" } else { "hide" });
        if visible && self.restore_fail {
            Err("restore")
        } else {
            Ok(())
        }
    }
    fn refresh_bounds(&self) -> Result<(), Self::Error> {
        self.calls.borrow_mut().push("bounds");
        if self.bounds_fail {
            Err("bounds")
        } else {
            Ok(())
        }
    }
}

fn surface(visible: bool, bounds_fail: bool, restore_fail: bool) -> FakeSurface {
    FakeSurface {
        visible,
        bounds_fail,
        restore_fail,
        calls: RefCell::default(),
    }
}

#[test]
fn refresh_preserves_hidden_overlay_visibility() {
    let surface = surface(false, false, false);
    refresh(&surface).unwrap();
    assert_eq!(*surface.calls.borrow(), ["bounds"]);
}

#[test]
fn visible_surface_is_rebound_and_restored_without_navigation() {
    let surface = surface(true, false, false);
    refresh(&surface).unwrap();
    assert_eq!(*surface.calls.borrow(), ["hide", "bounds", "restore"]);
}

#[test]
fn refresh_error_still_restores_the_original_visibility() {
    let surface = surface(true, true, false);
    assert_eq!(refresh(&surface), Err("bounds"));
    assert_eq!(*surface.calls.borrow(), ["hide", "bounds", "restore"]);
}

#[test]
fn restore_failure_is_not_reported_as_success() {
    let surface = surface(true, false, true);
    assert_eq!(refresh(&surface), Err("restore"));
}
