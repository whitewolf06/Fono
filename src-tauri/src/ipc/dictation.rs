//! IPC commands for observing and controlling the active dictation operation.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppResult;
use crate::pipeline::{self, Pipeline};
use crate::state::AppState;
use crate::types::PipelineState;

#[tauri::command]
pub fn get_pipeline_state(state: State<'_, AppState>) -> PipelineState {
    state.pipeline_state()
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
