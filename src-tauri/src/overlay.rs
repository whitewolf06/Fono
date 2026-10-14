//! Overlay recovery and temporary previews, independent of dictation operations.

use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::error::{AppError, AppResult};
use crate::overlay_geometry::GeometryState;
use crate::state::AppState;
use crate::types::Settings;

mod dispatch;
mod position;
mod visibility;
mod window_layout;

pub(crate) use visibility::request as request_visibility;

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
    positions: position::Queue,
}

/// Renderer-only display flags survive native settings/visibility synchronization.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayLayout {
    pub help_open: bool,
    #[serde(default)]
    pub error_visible: bool,
    #[serde(default)]
    pub content_height: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct OverlayDimensions {
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub async fn set_overlay_layout(
    app: AppHandle,
    layout: OverlayLayout,
) -> AppResult<OverlayDimensions> {
    dispatch::run(app.clone(), move || {
        let runtime = app.state::<OverlayRuntime>();
        {
            let mut geometry = runtime.geometry.lock();
            geometry.help_open = layout.help_open;
            geometry.error_visible = layout.error_visible;
            geometry.content_height =
                crate::overlay_geometry::valid_content_height(layout.content_height);
        }
        window_layout::apply(&app, &overlay_window(&app)?)
    })
    .await
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
pub async fn show_overlay_preview(
    app: AppHandle,
    overlay_scale: f32,
    overlay_opacity: f32,
    overlay_mini_mode: bool,
) -> AppResult<()> {
    validate_appearance(overlay_scale, overlay_opacity)?;
    let ticket = reserve_preview(&app.state::<OverlayRuntime>());
    show_preview_ticket(
        app,
        ticket,
        Some((overlay_scale, overlay_opacity, overlay_mini_mode)),
    )
    .await
}

async fn show_preview_ticket(
    app: AppHandle,
    ticket: u64,
    appearance: Option<(f32, f32, bool)>,
) -> AppResult<()> {
    let ui_app = app.clone();
    dispatch::run(app, move || {
        let runtime = ui_app.state::<OverlayRuntime>();
        if !preview_ticket_current(&runtime, ticket) {
            return Ok(());
        }
        let (overlay_scale, overlay_opacity, overlay_mini_mode) = appearance.unwrap_or_else(|| {
            let settings = ui_app.state::<AppState>().settings();
            (
                settings.overlay_scale,
                settings.overlay_opacity,
                settings.overlay_mini_mode,
            )
        });
        let preview = OverlayPreview {
            id: ticket,
            overlay_scale,
            overlay_opacity,
            overlay_mini_mode,
        };
        *runtime.preview.lock() = Some(preview);
        if let Err(error) =
            overlay_window(&ui_app).and_then(|window| show_window_on_ui(&ui_app, &window))
        {
            *runtime.preview.lock() = None;
            let _ = ui_app.emit("overlay-preview", Option::<OverlayPreview>::None);
            return Err(error);
        }
        // Cleanup belongs to the owned window task, even if its caller is cancelled.
        arm_preview_expiry(ui_app.clone(), ticket);
        ui_app.emit("overlay-preview", Some(preview))?;
        Ok(())
    })
    .await
}

fn arm_preview_expiry(app: AppHandle, id: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let ui_app = app.clone();
        let result = dispatch::run(app, move || {
            // Serialize expiry with showing a newer preview and its event.
            if clear_preview_if_current(&ui_app.state::<OverlayRuntime>(), id) {
                ui_app.emit("overlay-preview", Option::<OverlayPreview>::None)?;
                request_visibility(&ui_app);
            }
            Ok(())
        })
        .await;
        if result.is_err() {
            tracing::warn!("could not expire overlay preview on UI thread");
        }
    });
}

#[tauri::command]
pub async fn reset_overlay_position(app: AppHandle) -> AppResult<()> {
    // A later dismissal also invalidates a preview still waiting on persistence.
    let ticket = reserve_preview(&app.state::<OverlayRuntime>());
    let owned_app = app.clone();
    app.state::<OverlayRuntime>()
        .positions
        .run(async move {
            let ui_app = owned_app.clone();
            let (x, y) = dispatch::run(owned_app.clone(), move || {
                window_layout::place(&ui_app, &overlay_window(&ui_app)?, None)
            })
            .await?;
            persist_position(&owned_app, x, y)?;
            show_preview_ticket(owned_app, ticket, None).await
        })
        .await
}

pub fn restore_position(app: &AppHandle, _settings: &Settings) -> AppResult<()> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = position_operation(&app, |ui_app| {
            // Startup work may be delayed; use the latest saved preferences.
            let settings = ui_app.state::<AppState>().settings();
            window_layout::restore(ui_app, &overlay_window(ui_app)?, &settings)
        })
        .await;
        if result.is_err() {
            tracing::warn!("could not restore overlay position on UI thread");
        }
    });
    Ok(())
}

/// Called only inside dispatch::run's main-thread callback.
fn show_window_on_ui(app: &AppHandle, window: &WebviewWindow) -> AppResult<()> {
    window_layout::apply(app, window)?;
    window.unminimize()?;
    window.show()?;
    Ok(())
}

/// Called by the debounced renderer movement listener. Check the live window,
/// so hiding/minimizing and stale timers after a reset cannot overwrite its position.
pub async fn save_position(app: &AppHandle, x: i32, y: i32) -> AppResult<()> {
    position_operation(app, move |ui_app| {
        let window = overlay_window(ui_app)?;
        if !window.is_visible()? || window.is_minimized()? {
            return Ok(None);
        }
        let current = window.outer_position()?;
        if current.x != x || current.y != y || !window_layout::position_fits(&window, x, y)? {
            return Ok(None);
        }
        let position = ui_app
            .state::<OverlayRuntime>()
            .geometry
            .lock()
            .position_to_save((x, y));
        Ok(position)
    })
    .await
}

async fn position_operation(
    app: &AppHandle,
    work: impl FnOnce(&AppHandle) -> AppResult<Option<(i32, i32)>> + Send + 'static,
) -> AppResult<()> {
    let owned_app = app.clone();
    app.state::<OverlayRuntime>()
        .positions
        .run(async move {
            let ui_app = owned_app.clone();
            let position = dispatch::run(owned_app.clone(), move || work(&ui_app)).await?;
            if let Some((x, y)) = position {
                persist_position(&owned_app, x, y)?;
            }
            Ok(())
        })
        .await
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
pub async fn position_overlay(app: AppHandle, position: String) -> AppResult<()> {
    if position != "top" && position != "bottom" {
        return Err(AppError::Config("Неизвестное положение индикатора".into()));
    }
    position_operation(&app, move |ui_app| {
        window_layout::place(ui_app, &overlay_window(ui_app)?, Some(&position)).map(Some)
    })
    .await
}
#[tauri::command]
pub fn hide_overlay_preview(app: AppHandle) -> AppResult<()> {
    invalidate_preview(&app.state::<OverlayRuntime>());
    app.emit("overlay-preview", Option::<OverlayPreview>::None)?;
    request_visibility(&app);
    Ok(())
}

fn reserve_preview(runtime: &OverlayRuntime) -> u64 {
    runtime.next_id.fetch_add(1, Ordering::AcqRel) + 1
}

fn preview_ticket_current(runtime: &OverlayRuntime, id: u64) -> bool {
    runtime.next_id.load(Ordering::Acquire) == id
}

fn invalidate_preview(runtime: &OverlayRuntime) {
    reserve_preview(runtime);
    *runtime.preview.lock() = None;
}

fn clear_preview_if_current(runtime: &OverlayRuntime, id: u64) -> bool {
    let mut preview = runtime.preview.lock();
    if preview.as_ref().map(|preview| preview.id) != Some(id) {
        return false;
    }
    *preview = None;
    true
}
