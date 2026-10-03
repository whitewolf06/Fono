use crate::{
    application::updates::{self, UpdateSnapshot},
    error::{AppError, AppResult},
};
use tauri::{AppHandle, WebviewWindow};

fn require_main(window: &WebviewWindow) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Управление обновлениями доступно только в основном окне Fono.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateSnapshot {
    updates::status(&app)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle, window: WebviewWindow) -> AppResult<UpdateSnapshot> {
    require_main(&window)?;
    updates::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle, window: WebviewWindow) -> AppResult<UpdateSnapshot> {
    require_main(&window)?;
    updates::install(&app).await
}

#[tauri::command]
pub fn cancel_update_download(app: AppHandle, window: WebviewWindow) -> AppResult<UpdateSnapshot> {
    require_main(&window)?;
    Ok(updates::cancel(&app))
}

#[tauri::command]
pub async fn set_update_checks_enabled(
    app: AppHandle,
    window: WebviewWindow,
    enabled: bool,
) -> AppResult<UpdateSnapshot> {
    require_main(&window)?;
    updates::set_checks_enabled(&app, enabled).await
}
