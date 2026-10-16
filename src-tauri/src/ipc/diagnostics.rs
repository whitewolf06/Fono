//! Thin Tauri IPC facade for diagnostic use cases.

use tauri::AppHandle;

use crate::error::AppResult;

#[tauri::command]
pub async fn clear_logs() -> AppResult<()> {
    super::blocking::run(crate::application::diagnostics::clear_logs).await
}

#[tauri::command]
pub async fn get_recent_logs(lines: Option<usize>) -> AppResult<String> {
    super::blocking::run(move || crate::application::diagnostics::get_recent_logs(lines)).await
}

#[tauri::command]
pub async fn test_microphone(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<crate::application::diagnostics::MicTestResult> {
    crate::application::diagnostics::test_microphone(app, duration_ms).await
}
