//! Typed desktop bridge for the local transcription service monitor.

use tauri::{AppHandle, Manager};

use crate::application::service_control::{ServiceControl, ServiceStatus};
use crate::application::transcription_jobs::TranscriptionJob;
use crate::error::AppResult;

#[tauri::command]
pub async fn get_local_transcription_service_snapshot(app: AppHandle) -> AppResult<ServiceStatus> {
    // The snapshot includes persisted service history, not just queue memory.
    super::blocking::run(move || Ok(app.state::<ServiceControl>().snapshot())).await
}

#[tauri::command]
pub async fn cancel_local_transcription_job(
    app: AppHandle,
    id: String,
) -> AppResult<Option<TranscriptionJob>> {
    // A terminal job observer persists the cancellation before returning.
    super::blocking::run(move || Ok(app.state::<ServiceControl>().cancel_job(&id))).await
}

#[tauri::command]
pub async fn clear_local_transcription_history(app: AppHandle) -> AppResult<()> {
    super::blocking::run(move || {
        crate::service_history::clear()?;
        crate::events::emit_service_changed(&app);
        Ok(())
    })
    .await
}

/// Copies the local REST bearer token without exposing it to the webview.
#[tauri::command]
pub async fn copy_local_transcription_api_token() -> AppResult<()> {
    super::blocking::run(|| {
        let token = crate::state::transcription_api_token()?;
        crate::injection::copy_text(&token)
    })
    .await
}

#[tauri::command]
pub async fn set_local_transcription_service_enabled(
    app: AppHandle,
    enabled: bool,
) -> AppResult<()> {
    super::blocking::run(move || app.state::<ServiceControl>().set_enabled(&app, enabled)).await
}
