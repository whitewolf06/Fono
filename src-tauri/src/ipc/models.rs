//! Thin Tauri IPC facade for speech and wake model use cases.

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::state::AppState;
use crate::types::WhisperModelInfo;

#[tauri::command]
pub fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    crate::application::models::list_whisper_models()
}

#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    crate::application::models::download_whisper_model(app, size).await
}

#[tauri::command]
pub fn is_kws_model_downloaded() -> AppResult<bool> {
    crate::application::models::is_kws_model_downloaded()
}

#[tauri::command]
pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    crate::application::models::download_kws_model(app).await
}

#[tauri::command]
pub fn cancel_model_download(download_id: String) -> bool {
    crate::application::models::cancel_download(&download_id)
}

#[tauri::command]
pub fn set_whisper_model(state: State<'_, AppState>, path: String) -> AppResult<()> {
    crate::application::models::set_whisper_model(state.inner(), path)
}
