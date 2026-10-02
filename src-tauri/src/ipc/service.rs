//! Typed desktop bridge for the local transcription service monitor.

use tauri::{AppHandle, Manager};

use crate::application::service_control::{ServiceControl, ServiceStatus};
use crate::application::transcription_jobs::TranscriptionJob;
use crate::error::AppResult;

#[tauri::command]
pub fn get_local_transcription_service_snapshot(app: AppHandle) -> ServiceStatus {
    app.state::<ServiceControl>().snapshot()
}

#[tauri::command]
pub fn cancel_local_transcription_job(app: AppHandle, id: String) -> Option<TranscriptionJob> {
    app.state::<ServiceControl>().cancel_job(&id)
}

#[tauri::command]
pub fn clear_local_transcription_history(app: AppHandle) -> AppResult<()> {
    crate::service_history::clear()?;
    crate::events::emit_service_changed(&app);
    Ok(())
}

/// Copies the local REST bearer token without exposing it to the webview.
#[tauri::command]
pub fn copy_local_transcription_api_token() -> AppResult<()> {
    let token = crate::state::transcription_api_token()?;
    crate::injection::copy_text(&token)
}

#[tauri::command]
pub fn set_local_transcription_service_enabled(app: AppHandle, enabled: bool) -> AppResult<()> {
    app.state::<ServiceControl>().set_enabled(&app, enabled)
}
