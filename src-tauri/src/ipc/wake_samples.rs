//! Preparation is available while the detector itself remains unavailable.
use crate::{
    application::{dictation::wake_sample, wake_samples},
    error::AppResult,
};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn list_wake_samples() -> AppResult<Vec<wake_samples::Sample>> {
    super::blocking::run(wake_samples::list).await
}
#[tauri::command]
pub(crate) async fn start_wake_sample(
    app: AppHandle,
    request: wake_samples::SampleRequest,
) -> AppResult<wake_sample::Started> {
    let reservation = wake_sample::reserve(&app, request)?;
    wake_sample::start_reserved(app, reservation).await
}
#[tauri::command]
pub(crate) async fn finish_wake_sample(
    app: AppHandle,
    session_id: u64,
) -> AppResult<wake_samples::Sample> {
    super::blocking::run(move || wake_sample::finish(app, session_id)).await
}
#[tauri::command]
pub(crate) async fn cancel_wake_sample(app: AppHandle, session_id: u64) -> AppResult<()> {
    wake_sample::request_cancel(&app, session_id);
    super::blocking::run(move || wake_sample::cancel(&app, session_id)).await
}
#[tauri::command]
pub(crate) async fn read_wake_sample(id: String) -> AppResult<String> {
    super::blocking::run(move || wake_samples::read_audio(&id)).await
}
#[tauri::command]
pub(crate) async fn delete_wake_sample(id: String) -> AppResult<()> {
    super::blocking::run(move || wake_samples::delete(&id)).await
}
