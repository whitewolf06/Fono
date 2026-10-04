//! Native sizing never activates the overlay or persists temporary clamp offsets.
use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::{
    error::{AppError, AppResult},
    overlay_geometry::{
        center, display_mode, fit_at_anchor, fits_screen, logical_size, valid_dpi, ScreenRect,
    },
    state::AppState,
    types::{AiMode, HotkeyMode, PipelineState, Settings},
};

use super::{OverlayDimensions, OverlayRuntime};

pub(super) fn apply(app: &AppHandle, window: &WebviewWindow) -> AppResult<OverlayDimensions> {
    let settings = app.state::<AppState>().settings();
    let phase = app.state::<AppState>().pipeline_state();
    let preview = if phase == PipelineState::Idle {
        *app.state::<OverlayRuntime>().preview.lock()
    } else {
        None
    };
    let (scale, compact) = preview
        .map(|p| (p.overlay_scale, p.overlay_mini_mode))
        .unwrap_or((settings.overlay_scale, settings.overlay_mini_mode));
    let mode = display_mode(
        crate::application::dictation::workflow::has_pending(app),
        phase == PipelineState::Listening,
        preview.is_some(),
        settings.hotkey_mode == HotkeyMode::Toggle
            && settings.ai_mode != AiMode::Off
            && settings.overlay_quick_processing,
    );
    let runtime = app.state::<OverlayRuntime>();
    let mut geometry = runtime.geometry.lock();
    let current = window.outer_position()?;
    geometry.observe_position((current.x, current.y));
    let anchor = match geometry.anchor {
        Some(anchor) => anchor,
        None => {
            let anchor = (current.x, current.y);
            geometry.set_anchor(anchor);
            anchor
        }
    };
    let monitor = monitor_at_anchor(window, anchor)?;
    let screen = screen_rect(&monitor);
    if screen.width == 0 || screen.height == 0 {
        return Err(AppError::Internal(
            "Недоступна рабочая область экрана".into(),
        ));
    }
    let size = logical_size(
        mode,
        compact,
        geometry.help_open,
        geometry.error_visible,
        scale,
    );
    let rect = fit_at_anchor(anchor, size, monitor.scale_factor(), screen);
    let desired = PhysicalSize::new(rect.width, rect.height);
    if window.outer_size()? != desired {
        window.set_size(desired)?;
    }
    let position = PhysicalPosition::new(rect.x, rect.y);
    if window.outer_position()? != position {
        geometry.mark_programmatic_move((rect.x, rect.y));
        window.set_position(position)?;
    }
    geometry.remember_layout((rect.x, rect.y));
    let dpi = valid_dpi(monitor.scale_factor());
    Ok(OverlayDimensions {
        width: f64::from(rect.width) / dpi,
        height: f64::from(rect.height) / dpi,
    })
}

pub(super) fn screen_rect(monitor: &Monitor) -> ScreenRect {
    let area = monitor.work_area();
    ScreenRect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

fn monitor_at_anchor(window: &WebviewWindow, anchor: (i32, i32)) -> AppResult<Monitor> {
    window
        .available_monitors()?
        .into_iter()
        .find(|monitor| fits_screen(anchor.0, anchor.1, 1, 1, screen_rect(monitor)))
        .or(window.current_monitor()?)
        .or(window.primary_monitor()?)
        .ok_or_else(|| AppError::Internal("Не найден экран для оверлея".into()))
}

pub(super) fn position_fits(window: &WebviewWindow, x: i32, y: i32) -> AppResult<bool> {
    let size = window.outer_size()?;
    Ok(window
        .available_monitors()?
        .iter()
        .any(|monitor| fits_screen(x, y, size.width, size.height, screen_rect(monitor))))
}

pub(super) fn set_anchor(app: &AppHandle, position: (i32, i32)) {
    app.state::<OverlayRuntime>()
        .geometry
        .lock()
        .set_anchor(position);
}

pub(super) fn place(app: &AppHandle, window: &WebviewWindow, edge: Option<&str>) -> AppResult<()> {
    // Explicit reset/placement follows the settings window's monitor.
    let monitor = app
        .get_webview_window("settings")
        .and_then(|settings| settings.current_monitor().ok().flatten())
        .or(window.primary_monitor()?)
        .ok_or_else(|| AppError::Internal("Не найден экран для оверлея".into()))?;
    let screen = screen_rect(&monitor);
    set_anchor(app, (screen.x, screen.y));
    apply(app, window)?;
    let size = window.outer_size()?;
    let (x, centered_y) = center(size.width, size.height, screen);
    let margin = 24.min(screen.height.saturating_sub(size.height));
    let y = match edge {
        Some("top") => screen.y + margin as i32,
        Some("bottom") => screen.y + screen.height.saturating_sub(size.height + margin) as i32,
        _ => centered_y,
    };
    set_anchor(app, (x, y));
    apply(app, window)?;
    super::persist_position(app, x, y)
}

pub(super) fn restore(
    app: &AppHandle,
    window: &WebviewWindow,
    settings: &Settings,
) -> AppResult<()> {
    if let (Some(x), Some(y)) = (settings.overlay_x, settings.overlay_y) {
        if window
            .available_monitors()?
            .iter()
            .any(|m| fits_screen(x, y, 1, 1, screen_rect(m)))
        {
            set_anchor(app, (x, y));
            apply(app, window)?;
            return Ok(());
        }
    }
    place(app, window, None)
}
