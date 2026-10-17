use tauri::{AppHandle, Manager};

use crate::{state::AppState, types::PipelineState};

#[derive(Clone, Copy)]
pub(super) struct Snapshot {
    pub phase: PipelineState,
    pub enabled: bool,
    pub pending: bool,
    pub preview: bool,
}

impl Snapshot {
    pub fn visible(self) -> bool {
        (self.enabled && self.phase != PipelineState::Idle) || self.pending || self.preview
    }
}

fn read(app: &AppHandle) -> Snapshot {
    let state = app.state::<AppState>();
    Snapshot {
        phase: app.state::<crate::pipeline::Pipeline>().state(),
        enabled: state.settings().overlay_enabled,
        pending: crate::application::dictation::workflow::has_pending(app),
        preview: super::has_preview(app),
    }
}

/// Fire-and-forget: callers may hold a session fence, but never wait on the UI.
/// An old Idle request cannot hide a new recording or a newer preview.
pub(crate) fn request(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let read_app = app.clone();
        let apply_app = app.clone();
        let result = super::dispatch::run_latest(
            app,
            move || read(&read_app),
            move |snapshot| {
                let Some(window) = apply_app.get_webview_window("overlay") else {
                    return Ok(());
                };
                if snapshot.visible() {
                    super::show_window_on_ui(&apply_app, &window)
                } else {
                    window.hide().map_err(crate::error::AppError::from)
                }
            },
        )
        .await;
        if result.is_err() {
            // The operation was already committed; window failure cannot retain it.
            tracing::warn!("could not synchronize overlay visibility on UI thread");
        }
    });
}
