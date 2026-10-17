//! IPC commands for observing and controlling the active dictation operation.

use tauri::{AppHandle, Manager};

use crate::application::dictation::DictationOutcome;
use crate::error::{AppError, AppResult};
use crate::pipeline::{self, Pipeline};
use crate::state::AppState;
use fono_core::OperationSource;

#[tauri::command]
pub async fn start_dictation(app: AppHandle) -> AppResult<()> {
    super::blocking::run(move || crate::application::dictation::start(app, OperationSource::Ui))
        .await
}

#[tauri::command]
pub async fn stop_dictation(
    app: AppHandle,
    session_id: Option<u64>,
) -> AppResult<DictationOutcome> {
    if let Some(expected) = session_id {
        let operation = expected;
        if !app.state::<Pipeline>().is_operation_active(operation) {
            return Ok(DictationOutcome::interrupted(
                &app.state::<Pipeline>(),
                operation,
            ));
        }
        #[cfg(feature = "experimental-live")]
        if crate::application::live_dictation::is_active(&app) {
            return crate::application::live_dictation::finish_for(app, operation)
                .await
                .map(DictationOutcome::Completed);
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

/// Clicking the check icon produces a copyable result and never inserts it.
#[tauri::command]
pub async fn finish_overlay_dictation(
    app: AppHandle,
    session_id: u64,
) -> AppResult<DictationOutcome> {
    let operation = session_id;
    if !app.state::<Pipeline>().is_operation_active(operation) {
        return Ok(DictationOutcome::interrupted(
            &app.state::<Pipeline>(),
            operation,
        ));
    }
    crate::application::dictation::stop_overlay_for(app, operation).await
}
#[tauri::command]
pub fn acknowledge_overlay_processing_flush(
    app: AppHandle,
    session_id: u64,
    request_id: u64,
    error: Option<String>,
) {
    crate::application::dictation::workflow::acknowledge_overlay_flush(
        &app, session_id, request_id, error,
    );
}

#[tauri::command]
pub async fn transcribe_test(
    app: AppHandle,
    duration_ms: u64,
    inject: Option<bool>,
) -> AppResult<DictationOutcome> {
    crate::application::dictation::transcribe_test(app, duration_ms, inject).await
}

#[tauri::command]
pub fn confirm_dictation(app: AppHandle, session_id: Option<u64>) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.requested_operation(session_id)?;
    #[cfg(feature = "experimental-live")]
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
pub async fn cancel_dictation(app: AppHandle, session_id: Option<u64>) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.requested_operation(session_id)?;
    #[cfg(feature = "experimental-live")]
    if crate::application::live_dictation::is_active(&app) {
        // Resolve identity before queueing: this request must never cancel a
        // later session if teardown waits for another native operation.
        return super::blocking::run_admitted(activity, move || {
            crate::application::live_dictation::cancel_for(&app, operation)
        })
        .await;
    }
    if let Some(event) = pipeline.cancel_for(operation) {
        crate::events::emit_operation(&app, event);
    } else if session_id.is_some() {
        return Err(AppError::Cancelled("Диктовка отменена или заменена".into()));
    }
    // Fence late results immediately; native capture teardown may block.
    super::blocking::run_admitted(activity, move || {
        let state = app.state::<AppState>();
        let pipeline = app.state::<Pipeline>();
        crate::application::dictation::workflow::cancelled(&app, operation);
        let _ = pipeline.stop_recording_for(operation);
        pipeline::set_idle_if_no_operation(&app, state.inner(), &pipeline);
        crate::application::dictation::resume_wake_if_idle(&app);
        crate::events::emit_dictation_outcome(
            &app,
            operation,
            crate::application::dictation::DictationOutcomeKind::Cancelled,
        );
        tracing::info!("dictation cancelled by overlay");
        Ok(())
    })
    .await
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
) -> AppResult<DictationOutcome> {
    crate::application::dictation::workflow::resolve(app, request).await
}

#[tauri::command]
pub async fn update_overlay_processing_choice(
    app: AppHandle,
    request: crate::application::dictation::workflow::OverlayProcessingChoiceRequest,
) -> AppResult<crate::types::Settings> {
    crate::application::dictation::workflow::update_overlay_choice(app, request).await
}

#[cfg(feature = "experimental-live")]
#[tauri::command]
pub fn get_live_dictation(
    app: AppHandle,
) -> Option<crate::application::live_dictation::LiveSnapshot> {
    crate::application::live_dictation::snapshot(&app)
}

#[cfg(feature = "experimental-live")]
#[tauri::command]
pub async fn resume_live_insertion(app: AppHandle) -> AppResult<()> {
    crate::application::live_dictation::resume(app).await
}
