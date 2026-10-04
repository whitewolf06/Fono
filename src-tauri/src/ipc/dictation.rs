//! IPC commands for observing and controlling the active dictation operation.

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
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
    crate::application::dictation::start(app, OperationSource::Ui)
}

#[tauri::command]
pub async fn stop_dictation(app: AppHandle, session_id: Option<u64>) -> AppResult<Transcript> {
    if let Some(expected) = session_id {
        let operation = app
            .state::<Pipeline>()
            .requested_operation(Some(expected))?;
        if crate::application::live_dictation::is_active(&app) {
            return crate::application::live_dictation::finish_for(app, operation).await;
        }
        return crate::application::dictation::stop_with_reason_for(
            app,
            operation,
            crate::application::dictation_tail_diagnostics::DictationStopReason::Manual,
        )
        .await;
    }
    crate::application::dictation::stop(app).await
}

#[tauri::command]
pub async fn transcribe_test(
    app: AppHandle,
    duration_ms: u64,
    inject: Option<bool>,
) -> AppResult<Transcript> {
    crate::application::dictation::transcribe_test(app, duration_ms, inject).await
}

#[tauri::command]
pub fn confirm_dictation(app: AppHandle, session_id: Option<u64>) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.requested_operation(session_id)?;
    if crate::application::live_dictation::is_active(&app) {
        tauri::async_runtime::spawn(async move {
            let _ = crate::application::live_dictation::finish_for(app, operation).await;
        });
        return Ok(());
    }
    if !pipeline.confirm_for(operation) && session_id.is_some() {
        return Err(AppError::Cancelled("Диктовка отменена или заменена".into()));
    }
    tracing::info!("dictation confirmed by overlay");
    Ok(())
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle, session_id: Option<u64>) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.requested_operation(session_id)?;
    if crate::application::live_dictation::is_active(&app) {
        return if session_id.is_some() {
            crate::application::live_dictation::cancel_for(&app, operation)
        } else {
            crate::application::live_dictation::cancel(&app)
        };
    }
    let state = app.state::<AppState>();
    if let Some(event) = pipeline.cancel_for(operation) {
        crate::events::emit_operation(&app, event);
    } else if session_id.is_some() {
        return Err(AppError::Cancelled("Диктовка отменена или заменена".into()));
    }
    crate::application::dictation::workflow::cancelled(&app, operation);
    let _ = pipeline.stop_recording_for(operation);
    pipeline::set_idle_if_no_operation(&app, state.inner(), &pipeline);
    crate::application::dictation::resume_wake_if_idle(&app);
    tracing::info!("dictation cancelled by overlay");
    Ok(())
}

#[tauri::command]
pub fn dismiss_dictation_overlay(app: AppHandle) -> AppResult<()> {
    crate::application::dictation::workflow::dismiss_overlay(&app)
}

#[tauri::command]
pub fn get_pending_dictation(
    app: AppHandle,
) -> Option<crate::application::dictation::workflow::PendingDictation> {
    crate::application::dictation::workflow::snapshot(&app)
}

#[tauri::command]
pub async fn resolve_pending_dictation(
    app: AppHandle,
    request: crate::application::dictation::workflow::PendingRequest,
) -> AppResult<Option<Transcript>> {
    crate::application::dictation::workflow::resolve(app, request).await
}

#[tauri::command]
pub async fn update_overlay_processing_choice(
    app: AppHandle,
    request: crate::application::dictation::workflow::OverlayProcessingChoiceRequest,
) -> AppResult<crate::types::Settings> {
    crate::application::dictation::workflow::update_overlay_choice(app, request).await
}

#[tauri::command]
pub fn get_live_dictation(
    app: AppHandle,
) -> Option<crate::application::live_dictation::LiveSnapshot> {
    crate::application::live_dictation::snapshot(&app)
}

#[tauri::command]
pub async fn resume_live_insertion(app: AppHandle) -> AppResult<()> {
    crate::application::live_dictation::resume(app).await
}
