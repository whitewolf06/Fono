//! Overlay recovery and temporary previews, independent of dictation operations.

use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::error::{AppError, AppResult};
use crate::overlay_geometry::GeometryState;
use crate::state::AppState;
use crate::types::Settings;

mod window_layout;

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
    geometry: Mutex<GeometryState>,
}

/// Renderer-only display flags survive native settings/visibility synchronization.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayLayout {
    pub help_open: bool,
    #[serde(default)]
    pub error_visible: bool,
}

#[derive(Debug, Serialize)]
pub struct OverlayDimensions {
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub fn set_overlay_layout(app: AppHandle, layout: OverlayLayout) -> AppResult<OverlayDimensions> {
    let runtime = app.state::<OverlayRuntime>();
    {
        let mut geometry = runtime.geometry.lock();
        geometry.help_open = layout.help_open;
        geometry.error_visible = layout.error_visible;
    }
    window_layout::apply(&app, &overlay_window(&app)?)
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
    window_layout::place(&app, &window, None)?;
    show_overlay_preview(
        app,
        settings.overlay_scale,
        settings.overlay_opacity,
        settings.overlay_mini_mode,
    )
}

pub fn restore_position(app: &AppHandle, settings: &Settings) -> AppResult<()> {
    let window = overlay_window(app)?;
    window_layout::restore(app, &window, settings)
}

pub fn show_window(app: &AppHandle) -> AppResult<()> {
    let window = overlay_window(app)?;
    window_layout::apply(app, &window)?;
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
    if current.x != x || current.y != y || !window_layout::position_fits(&window, x, y)? {
        return Ok(());
    }
    let position = app
        .state::<OverlayRuntime>()
        .geometry
        .lock()
        .position_to_save((x, y));
    let Some((anchor_x, anchor_y)) = position else {
        return Ok(());
    };
    persist_position(app, anchor_x, anchor_y)
}

fn overlay_window(app: &AppHandle) -> AppResult<WebviewWindow> {
    app.get_webview_window("overlay")
        .ok_or_else(|| AppError::Internal("Окно оверлея недоступно".into()))
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
    window_layout::place(&app, &window, Some(&position))
}
#[tauri::command]
pub fn hide_overlay_preview(app: AppHandle) -> AppResult<()> {
    *app.state::<OverlayRuntime>().preview.lock() = None;
    app.emit("overlay-preview", Option::<OverlayPreview>::None)?;
    crate::pipeline::sync_overlay_window(&app, app.state::<AppState>().pipeline_state());
    Ok(())
}
