//! Typed desktop bridge for the local transcription service monitor.

use tauri::{AppHandle, Manager};

use crate::application::local_transcription_service::{
    LocalTranscriptionService, LocalTranscriptionServiceSnapshot,
};
use crate::application::transcription_jobs::TranscriptionJob;
use crate::error::AppResult;

#[tauri::command]
pub fn get_local_transcription_service_snapshot(
    app: AppHandle,
) -> LocalTranscriptionServiceSnapshot {
    app.state::<LocalTranscriptionService>().snapshot()
}

#[tauri::command]
pub fn cancel_local_transcription_job(app: AppHandle, id: String) -> Option<TranscriptionJob> {
    app.state::<LocalTranscriptionService>().cancel_job(&id)
}

#[tauri::command]
pub fn clear_local_transcription_history(app: AppHandle) -> AppResult<()> {
    app.state::<LocalTranscriptionService>().clear_history()
}

/// Copies the local REST bearer token without exposing it to the webview.
#[tauri::command]
pub fn copy_local_transcription_api_token() -> AppResult<()> {
    let token = crate::state::transcription_api_token()?;
    crate::injection::copy_text(&token)
}
