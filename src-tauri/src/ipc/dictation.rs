//! IPC commands for observing and controlling the active dictation operation.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppResult;
use crate::operation::OperationSource;
use crate::pipeline::{self, Pipeline};
use crate::state::AppState;
use crate::types::PipelineState;
use crate::types::Transcript;

#[tauri::command]
pub fn get_pipeline_state(state: State<'_, AppState>) -> PipelineState {
    state.pipeline_state()
}

#[tauri::command]
pub fn start_dictation(app: AppHandle) -> AppResult<()> {
    crate::commands::start_dictation_from(app, OperationSource::Ui)
}

pub(crate) fn start_dictation_from(app: AppHandle, source: OperationSource) -> AppResult<()> {
    crate::commands::start_dictation_from(app, source)
}

#[tauri::command]
pub async fn stop_dictation(app: AppHandle) -> AppResult<Transcript> {
    crate::commands::stop_dictation(app).await
}

#[tauri::command]
pub async fn transcribe_test(
    app: AppHandle,
    duration_ms: u64,
    inject: Option<bool>,
) -> AppResult<Transcript> {
    crate::commands::transcribe_test(app, duration_ms, inject).await
}

#[tauri::command]
pub fn confirm_dictation(app: AppHandle) -> AppResult<()> {
    app.state::<Pipeline>().confirm();
    tracing::info!("dictation confirmed by overlay");
    Ok(())
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    if let Some(event) = pipeline.cancel() {
        let _ = app.emit("operation-state", event);
    }
    let _ = pipeline.stop_recording();
    pipeline::set_state(&app, state.inner(), PipelineState::Idle);
    app.state::<fono_wake::WakeWordHandle>().resume();
    tracing::info!("dictation cancelled by overlay");
    Ok(())
}
