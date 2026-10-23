//! Preparation is available while the detector itself remains unavailable.
use crate::{
    application::{dictation::wake_sample, wake_samples},
    error::AppResult,
};
use tauri::AppHandle;
use tauri::Manager;

#[tauri::command]
pub(crate) fn get_lira_live_status(app: AppHandle) -> crate::application::wake_lira::Snapshot {
    app.state::<crate::application::wake_lira::Runtime>()
        .snapshot()
}
#[tauri::command]
pub(crate) async fn start_lira_live(
    app: AppHandle,
) -> AppResult<crate::application::wake_lira::Snapshot> {
    app.state::<crate::application::wake_lira::Runtime>()
        .start(&app)
}
#[tauri::command]
pub(crate) async fn stop_lira_live(
    app: AppHandle,
    session_id: u64,
) -> AppResult<crate::application::wake_lira::Snapshot> {
    crate::application::wake_lira::stop(app, session_id).await
}

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

#[tauri::command]
pub(crate) fn wake_sample_test_available() -> bool {
    wake_samples::testing::available()
}
#[tauri::command]
pub(crate) async fn test_wake_sample(
    app: AppHandle,
    id: String,
) -> AppResult<wake_samples::testing::Result> {
    crate::application::dictation::ensure_capture_allowed(&app)?;
    if app
        .state::<crate::application::wake_lira::Runtime>()
        .snapshot()
        .active()
    {
        return Err(crate::error::AppError::Busy(
            "Сначала выключите живое пробуждение.".into(),
        ));
    }
    let reservation = wake_samples::testing::reserve()?;
    super::blocking::run(move || wake_samples::testing::run(&id, reservation)).await
}
