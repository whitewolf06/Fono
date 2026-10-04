//! Session-bound capture used exclusively by the postprocessing test editor.
use crate::{application::dictation, error::AppResult};
use tauri::AppHandle;

#[tauri::command]
pub(crate) fn start_processing_test_capture(
    app: AppHandle,
) -> AppResult<dictation::ProcessingCaptureStarted> {
    dictation::start_processing_test_capture(app)
}

#[tauri::command]
pub(crate) async fn finish_processing_test_capture(
    app: AppHandle,
    session_id: u64,
) -> AppResult<dictation::ProcessingCaptureResult> {
    dictation::finish_processing_test_capture(app, session_id).await
}

#[tauri::command]
pub(crate) fn cancel_processing_test_capture(app: AppHandle, session_id: u64) -> AppResult<()> {
    dictation::cancel_processing_test_capture(&app, session_id)
}
