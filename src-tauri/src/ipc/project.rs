use crate::error::{AppError, AppResult};
use tauri::WebviewWindow;

#[tauri::command]
pub async fn open_project_site(window: WebviewWindow) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Сайт проекта можно открыть из основного окна Fono.".into(),
        ));
    }
    tauri::async_runtime::spawn_blocking(crate::application::project_site::open)
        .await
        .map_err(|_| AppError::Internal("Не удалось открыть браузер.".into()))?
}
