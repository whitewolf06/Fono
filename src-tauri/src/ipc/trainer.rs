use crate::{
    application::{trainer_ai, trainer_report::TrainerReport, updates::activity},
    error::{AppError, AppResult},
    state::AppState,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn get_trainer_report(
    app: AppHandle,
    days: u32,
    refresh: bool,
) -> AppResult<TrainerReport> {
    let lease = activity::lease()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let state = app.state::<AppState>();
        let settings = state.settings.lock();
        trainer_ai::report(&crate::history::for_app(&app), days, refresh, &settings)
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось построить локальный отчёт".into()))?
}

#[tauri::command]
pub async fn analyze_trainer_period(
    app: AppHandle,
    days: u32,
    fingerprint: String,
    share_text: bool,
    request_id: String,
) -> AppResult<TrainerReport> {
    trainer_ai::analyze(app, days, fingerprint, share_text, request_id).await
}

#[tauri::command]
pub fn cancel_trainer_period(request_id: String) {
    trainer_ai::cancel(&request_id);
}
