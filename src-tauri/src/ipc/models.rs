//! Thin Tauri IPC facade for speech and wake model use cases.

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::state::AppState;
use crate::types::WhisperModelInfo;

#[tauri::command]
pub async fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    super::blocking::run(crate::application::models::list_whisper_models).await
}

#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    crate::application::models::download_whisper_model(app, size).await
}

#[cfg(feature = "wake-word")]
#[tauri::command]
pub async fn is_kws_model_downloaded(app: AppHandle) -> AppResult<bool> {
    super::blocking::run(move || crate::application::models::is_kws_model_downloaded(&app)).await
}

#[cfg(feature = "wake-word")]
#[tauri::command]
pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    crate::application::models::download_kws_model(app).await
}

#[tauri::command]
pub fn cancel_model_download(download_id: String) -> bool {
    crate::application::models::cancel_download(&download_id)
}

#[tauri::command]
pub async fn set_whisper_model(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<()> {
    crate::application::models::set_whisper_model(&app, state.inner(), path).await
}
