//! Overlay recovery and temporary previews, independent of dictation operations.

use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow};

use crate::error::{AppError, AppResult};
use crate::overlay_geometry::{center, fits_screen, ScreenRect};
use crate::state::AppState;
use crate::types::Settings;

#[derive(Clone, Copy, Serialize)]
pub struct OverlayPreview {
    id: u64,
    pub overlay_scale: f32,
    pub overlay_opacity: f32,
    pub overlay_mini_mode: bool,
}

#[derive(Default)]
pub struct OverlayRuntime {
    next_id: AtomicU64,
    preview: Mutex<Option<OverlayPreview>>,
}

#[tauri::command]
pub fn get_overlay_preview(runtime: State<'_, OverlayRuntime>) -> Option<OverlayPreview> {
    *runtime.preview.lock()
}

pub fn has_preview(app: &AppHandle) -> bool {
    app.state::<OverlayRuntime>().preview.lock().is_some()
}

pub fn validate_appearance(scale: f32, opacity: f32) -> AppResult<()> {
    if !scale.is_finite()
        || !(0.5..=2.0).contains(&scale)
        || !opacity.is_finite()
        || !(0.1..=1.0).contains(&opacity)
    {
        return Err(AppError::Config(
            "Некорректный масштаб или непрозрачность оверлея".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn show_overlay_preview(
    app: AppHandle,
    overlay_scale: f32,
    overlay_opacity: f32,
    overlay_mini_mode: bool,
) -> AppResult<()> {
    validate_appearance(overlay_scale, overlay_opacity)?;
    let runtime = app.state::<OverlayRuntime>();
    let id = runtime.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    *runtime.preview.lock() = Some(OverlayPreview {
        id,
        overlay_scale,
        overlay_opacity,
        overlay_mini_mode,
    });
    if let Err(error) = show_window(&app) {
        *runtime.preview.lock() = None;
        let _ = app.emit("overlay-preview", Option::<OverlayPreview>::None);
        return Err(error);
    }
    app.emit("overlay-preview", *runtime.preview.lock())?;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let runtime = app.state::<OverlayRuntime>();
        {
            let mut preview = runtime.preview.lock();
            // An older timer must not dismiss a newer preview.
            if preview.as_ref().map(|preview| preview.id) != Some(id) {
                return;
            }
            *preview = None;
        }
        let _ = app.emit("overlay-preview", Option::<OverlayPreview>::None);
        crate::pipeline::sync_overlay_window(&app, app.state::<AppState>().pipeline_state());
    });
    Ok(())
}

#[tauri::command]
pub fn reset_overlay_position(app: AppHandle) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    let window = overlay_window(&app)?;
    resize(&window, settings.overlay_scale)?;
    move_to_center(&app, &window)?;
    show_overlay_preview(
        app,
        settings.overlay_scale,
        settings.overlay_opacity,
        settings.overlay_mini_mode,
    )
}

pub fn restore_position(app: &AppHandle, settings: &Settings) -> AppResult<()> {
    let window = overlay_window(app)?;
    resize(&window, settings.overlay_scale)?;
    if let (Some(x), Some(y)) = (settings.overlay_x, settings.overlay_y) {
        if position_fits(&window, x, y)? {
            window.set_position(PhysicalPosition::new(x, y))?;
            return Ok(());
        }
    }
    move_to_center(app, &window)
}

pub fn show_window(app: &AppHandle) -> AppResult<()> {
    let window = overlay_window(app)?;
    let settings = app.state::<AppState>().settings();
    let scale = if app.state::<AppState>().pipeline_state() == crate::types::PipelineState::Idle {
        app.state::<OverlayRuntime>()
            .preview
            .lock()
            .map(|preview| preview.overlay_scale)
            .unwrap_or(settings.overlay_scale)
    } else {
        settings.overlay_scale
    };
    resize(&window, scale)?;
    let position = window.outer_position()?;
    if !position_fits(&window, position.x, position.y)? {
        move_to_center(app, &window)?;
    }
    window.unminimize()?;
    window.show()?;
    Ok(())
}

/// Called by the debounced renderer movement listener. Check the live window,
/// so hiding/minimizing and stale timers after a reset cannot overwrite its position.
pub fn save_position(app: &AppHandle, x: i32, y: i32) -> AppResult<()> {
    let window = overlay_window(app)?;
    if !window.is_visible()? || window.is_minimized()? {
        return Ok(());
    }
    let current = window.outer_position()?;
    if current.x != x || current.y != y || !position_fits(&window, x, y)? {
        return Ok(());
    }
    persist_position(app, x, y)
}

fn overlay_window(app: &AppHandle) -> AppResult<WebviewWindow> {
    app.get_webview_window("overlay")
        .ok_or_else(|| AppError::Internal("Окно оверлея недоступно".into()))
}

fn resize(window: &WebviewWindow, scale: f32) -> AppResult<()> {
    let scale = if scale.is_finite() {
        scale.clamp(0.5, 2.0)
    } else {
        1.0
    };
    window.set_size(tauri::LogicalSize::new(
        (286.0 * scale).round(),
        (88.0 * scale).round(),
    ))?;
    Ok(())
}

fn screen_rect(monitor: &tauri::Monitor) -> ScreenRect {
    let area = monitor.work_area();
    ScreenRect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

fn position_fits(window: &WebviewWindow, x: i32, y: i32) -> AppResult<bool> {
    let size = window.outer_size()?;
    Ok(window
        .available_monitors()?
        .iter()
        .any(|monitor| fits_screen(x, y, size.width, size.height, screen_rect(monitor))))
}

fn move_to_center(app: &AppHandle, window: &WebviewWindow) -> AppResult<()> {
    // Prefer the monitor containing settings, where the user pressed Reset.
    let monitor = app
        .get_webview_window("settings")
        .and_then(|settings| settings.current_monitor().ok().flatten())
        .or(window.primary_monitor()?)
        .ok_or_else(|| AppError::Internal("Не найден экран для оверлея".into()))?;
    let size = window.outer_size()?;
    let (x, y) = center(size.width, size.height, screen_rect(&monitor));
    window.set_position(PhysicalPosition::new(x, y))?;
    persist_position(app, x, y)?;
    tracing::info!(x, y, "overlay moved to screen center");
    Ok(())
}

fn persist_position(app: &AppHandle, x: i32, y: i32) -> AppResult<()> {
    let state = app.state::<AppState>();
    let base = state.settings();
    if base.overlay_x == Some(x) && base.overlay_y == Some(y) {
        return Ok(());
    }
    let mut settings = base.clone();
    settings.overlay_x = Some(x);
    settings.overlay_y = Some(y);
    state.persist_settings_delta(&base, &settings)?;
    Ok(())
}

#[tauri::command]
pub fn position_overlay(app: AppHandle, position: String) -> AppResult<()> {
    if position != "top" && position != "bottom" {
        return Err(AppError::Config("Неизвестное положение индикатора".into()));
    }
    let window = overlay_window(&app)?;
    let settings = app.state::<AppState>().settings();
    resize(&window, settings.overlay_scale)?;
    let monitor = app
        .get_webview_window("settings")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or(window.primary_monitor()?)
        .ok_or_else(|| AppError::Internal("Не найден монитор".into()))?;
    let screen = screen_rect(&monitor);
    let size = window.outer_size()?;
    let (x, _) = center(size.width, size.height, screen);
    let y = if position == "top" {
        screen.y + 24
    } else {
        screen.y + screen.height.saturating_sub(size.height + 24) as i32
    };
    window.set_position(PhysicalPosition::new(x, y))?;
    persist_position(&app, x, y)?;
    Ok(())
}
#[tauri::command]
pub fn hide_overlay_preview(app: AppHandle) -> AppResult<()> {
    *app.state::<OverlayRuntime>().preview.lock() = None;
    app.emit("overlay-preview", Option::<OverlayPreview>::None)?;
    crate::pipeline::sync_overlay_window(&app, app.state::<AppState>().pipeline_state());
    Ok(())
}
