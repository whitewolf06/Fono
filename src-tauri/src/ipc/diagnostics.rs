//! Thin Tauri IPC facade for diagnostic use cases.

use tauri::AppHandle;

use crate::error::AppResult;

#[tauri::command]
pub fn clear_logs() -> AppResult<()> {
    crate::application::diagnostics::clear_logs()
}

#[tauri::command]
pub fn get_recent_logs(lines: Option<usize>) -> AppResult<String> {
    crate::application::diagnostics::get_recent_logs(lines)
}

#[tauri::command]
pub async fn test_microphone(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<crate::application::diagnostics::MicTestResult> {
    crate::application::diagnostics::test_microphone(app, duration_ms).await
}
