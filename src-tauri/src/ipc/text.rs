use tauri::AppHandle;

use crate::error::AppResult;

#[tauri::command]
pub async fn copy_dictation_text(text: String) -> AppResult<()> {
    // The clipboard object is created, used and dropped by the blocking job.
    super::blocking::run(move || crate::injection::copy_text(&text)).await
}

#[tauri::command]
pub async fn reinsert_dictation(app: AppHandle, session_id: u64, text: String) -> AppResult<()> {
    crate::application::dictation::workflow::reinsert(app, session_id, text).await
}
