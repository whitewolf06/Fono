use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn copy_dictation_text(text: String) -> AppResult<()> {
    // The clipboard object is created, used and dropped by the blocking job.
    super::blocking::run(move || crate::injection::copy_text(&text)).await
}

#[tauri::command]
pub async fn reinsert_dictation(app: AppHandle, text: String) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    super::blocking::run(move || crate::injection::inject_text(&text, settings.injection_mode))
        .await
}
