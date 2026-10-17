//! Closing terminal/preview UI is distinct from cancelling a dictation.
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    types::PipelineState,
};
use tauri::{AppHandle, Manager};

pub(crate) fn dismiss(app: &AppHandle) -> AppResult<()> {
    app.state::<Pipeline>().dismiss_overlay_when_idle(
        || super::has_pending(app),
        || {
            if app.get_webview_window("overlay").is_none() {
                return Err(AppError::Internal("Окно оверлея недоступно".into()));
            }
            // Only the coordinator's idle projection can reach this closure.
            crate::events::emit_pipeline_state(app, PipelineState::Idle);
            crate::overlay::hide_overlay_preview(app.clone())
        },
    )
}
