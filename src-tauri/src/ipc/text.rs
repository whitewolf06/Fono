use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub fn copy_dictation_text(text: String) -> AppResult<()> {
    crate::injection::copy_text(&text)
}

#[tauri::command]
pub fn reinsert_dictation(app: AppHandle, text: String) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    crate::injection::inject_text(&text, settings.injection_mode)
}
