//! Closing terminal/preview UI is distinct from cancelling a dictation.
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    types::PipelineState,
};
use tauri::{AppHandle, Manager};

pub(crate) fn dismiss(app: &AppHandle) -> AppResult<()> {
    app.state::<Pipeline>().dismiss_overlay_when_idle(
        || super::has_pending(app),
        || {
            let window = app
                .get_webview_window("overlay")
                .ok_or_else(|| AppError::Internal("Окно оверлея недоступно".into()))?;
            window.hide()?;
            // A preview timer or settings update must not reopen a terminal
            // indicator because AppState still held the previous Error phase.
            app.state::<AppState>()
                .set_pipeline_state(PipelineState::Idle);
            crate::events::emit_pipeline_state(app, PipelineState::Idle);
            crate::overlay::hide_overlay_preview(app.clone())
        },
    )
}
