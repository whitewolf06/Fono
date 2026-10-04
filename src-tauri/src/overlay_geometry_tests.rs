use super::*;

const PRIMARY: ScreenRect = ScreenRect {
    x: 0,
    y: 0,
    width: 1920,
    height: 1040,
};

#[test]
fn rejects_minimized_and_disconnected_monitor_positions() {
    assert!(!fits_screen(-32000, -32000, 360, 104, PRIMARY));
    assert!(!fits_screen(2200, 40, 360, 104, PRIMARY));
    assert!(!fits_screen(1800, 40, 360, 104, PRIMARY));
    assert!(fits_screen(560, 40, 360, 104, PRIMARY));
}

#[test]
fn keeps_valid_negative_coordinates_on_a_secondary_monitor() {
    let left = ScreenRect {
        x: -1920,
        ..PRIMARY
    };
    assert!(fits_screen(-1500, 40, 360, 104, left));
    assert_eq!(center(360, 104, left), (-1140, 468));
    let rect = fit_at_anchor((-300, 950), (440.0, 412.0), 1.0, left);
    assert_eq!((rect.x, rect.y), (-440, 628));
    assert!(fits_screen(rect.x, rect.y, rect.width, rect.height, left));
}

#[test]
fn quick_choices_exist_only_during_listening_and_pending_takes_priority() {
    assert_eq!(
        display_mode(false, true, false, true),
        DisplayMode::QuickProcessing
    );
    assert_eq!(
        display_mode(false, false, false, true),
        DisplayMode::Toolbar
    );
    assert_eq!(
        display_mode(false, true, false, false),
        DisplayMode::Toolbar
    );
    assert_eq!(display_mode(true, true, false, true), DisplayMode::Pending);
    assert_eq!(
        display_mode(true, false, false, false),
        DisplayMode::Pending
    );
}

#[test]
fn idle_preview_gets_quick_space_only_when_the_controls_are_enabled() {
    assert_eq!(
        display_mode(false, false, true, true),
        DisplayMode::QuickProcessing
    );
    assert_eq!(
        display_mode(false, false, true, false),
        DisplayMode::Toolbar
    );
    assert_eq!(display_mode(true, false, true, true), DisplayMode::Pending);
}

#[test]
fn resize_before_drag_debounce_keeps_the_new_position_and_ignores_native_clamps() {
    let mut state = GeometryState::default();
    state.set_anchor((400, 200));
    state.remember_layout((400, 200));
    let dragged = (1530, 920);
    state.observe_position(dragged);
    assert_eq!(state.anchor, Some(dragged));
    let expanded = fit_at_anchor(state.anchor.unwrap(), (440.0, 412.0), 1.0, PRIMARY);
    state.mark_programmatic_move((expanded.x, expanded.y));
    state.observe_position((expanded.x, expanded.y));
    assert_eq!(state.anchor, Some(dragged));
    assert_eq!(
        state.position_to_save((expanded.x, expanded.y)),
        Some(dragged)
    );
    let collapsed = fit_at_anchor(state.anchor.unwrap(), (360.0, 104.0), 1.0, PRIMARY);
    assert_eq!((collapsed.x, collapsed.y), dragged);
    state.observe_position((-32000, -32000));
    assert_eq!(state.anchor, Some(dragged));
}

#[test]
fn toolbar_errors_and_help_reserve_space_without_expanding_pending_twice() {
    assert_eq!(
        logical_size(DisplayMode::Toolbar, false, false, false, 1.0),
        (360.0, 104.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Toolbar, true, false, false, 1.0),
        (320.0, 72.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Toolbar, false, true, true, 1.0),
        (360.0, 272.0)
    );
    assert_eq!(
        logical_size(DisplayMode::QuickProcessing, false, false, true, 1.0),
        (440.0, 192.0)
    );
    assert_eq!(
        logical_size(DisplayMode::QuickProcessing, true, true, true, 1.0),
        (400.0, 288.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Pending, false, true, true, 1.0),
        (440.0, 412.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Pending, true, false, true, 1.0),
        (400.0, 272.0)
    );
}

#[test]
fn combining_user_scale_and_monitor_dpi_clamps_to_available_work_area() {
    let screen = ScreenRect {
        x: 90,
        y: -1000,
        width: 800,
        height: 600,
    };
    let logical = logical_size(DisplayMode::Pending, false, true, true, 2.0);
    let rect = fit_at_anchor((750, -500), logical, 1.5, screen);
    assert_eq!(
        rect,
        WindowRect {
            x: 90,
            y: -1000,
            width: 800,
            height: 600
        }
    );
    assert!(fits_screen(rect.x, rect.y, rect.width, rect.height, screen));
    let regular = fit_at_anchor((90, -1000), (360.0, 104.0), 1.25, screen);
    assert_eq!((regular.width, regular.height), (450, 130));
}

#[test]
fn invalid_scaling_falls_back_and_supported_user_scale_is_bounded() {
    assert_eq!(
        logical_size(DisplayMode::Toolbar, false, false, false, f32::NAN),
        (360.0, 104.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Toolbar, false, false, false, 9.0),
        (720.0, 208.0)
    );
    assert_eq!(
        logical_size(DisplayMode::Toolbar, false, false, false, 0.0),
        (180.0, 52.0)
    );
    assert_eq!(
        fit_at_anchor((100, 100), (360.0, 104.0), f64::NAN, PRIMARY).width,
        360
    );
    assert_eq!(valid_dpi(0.0), 1.0);
}

#[test]
fn expansion_callbacks_never_replace_anchor_and_collapse_returns_without_drift() {
    let mut state = GeometryState::default();
    let anchor = (1530, 920);
    state.set_anchor(anchor);
    for _ in 0..8 {
        let expanded = fit_at_anchor(state.anchor.unwrap(), (440.0, 412.0), 1.0, PRIMARY);
        assert_eq!((expanded.x, expanded.y), (1480, 628));
        state.mark_programmatic_move((expanded.x, expanded.y));
        assert_eq!(state.position_to_save((expanded.x, expanded.y)), None);
        assert_eq!(state.position_to_save((expanded.x, expanded.y)), None);
        assert_eq!(state.anchor, Some(anchor));
        let collapsed = fit_at_anchor(state.anchor.unwrap(), (360.0, 104.0), 1.0, PRIMARY);
        assert_eq!((collapsed.x, collapsed.y), anchor);
        state.mark_programmatic_move((collapsed.x, collapsed.y));
        assert_eq!(state.position_to_save((collapsed.x, collapsed.y)), None);
    }
    assert_eq!(state.position_to_save((400, 220)), Some((400, 220)));
    assert_eq!(state.anchor, Some((400, 220)));
}

#[test]
fn resize_callback_saves_the_manual_anchor_once_instead_of_the_temporary_clamp() {
    let mut state = GeometryState::default();
    state.set_anchor((400, 200));
    state.remember_layout((400, 200));
    let manual = (1530, 920);
    state.observe_position(manual);
    let enlarged = fit_at_anchor(manual, (440.0, 412.0), 1.0, PRIMARY);
    let clamped = (enlarged.x, enlarged.y);
    assert_ne!(clamped, manual);
    state.mark_programmatic_move(clamped);
    assert_eq!(state.position_to_save(clamped), Some(manual));
    assert_eq!(state.position_to_save(clamped), None);
    assert_eq!(state.anchor, Some(manual));

    // Reset supersedes an unsaved gesture; its callback cannot revive that drag.
    state.observe_position((900, 700));
    state.set_anchor((100, 100));
    state.mark_programmatic_move((100, 100));
    assert_eq!(state.position_to_save((100, 100)), None);
}

#[test]
fn ordinary_layout_updates_retain_renderer_help_and_error_flags() {
    let mut state = GeometryState {
        help_open: true,
        error_visible: true,
        ..Default::default()
    };
    state.set_anchor((400, 200));
    state.mark_programmatic_move((400, 200));
    assert!(state.help_open);
    assert!(state.error_visible);
    assert_eq!(
        logical_size(
            DisplayMode::Toolbar,
            false,
            state.help_open,
            state.error_visible,
            1.0
        ),
        (360.0, 272.0)
    );
}
