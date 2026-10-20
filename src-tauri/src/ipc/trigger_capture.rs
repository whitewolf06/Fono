use crate::error::{AppError, AppResult};
use tauri::{AppHandle, WebviewWindow};

#[tauri::command]
pub async fn begin_trigger_capture(app: AppHandle, window: WebviewWindow) -> AppResult<String> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Выбор кнопки доступен в настройках Fono.".into(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let activity = crate::application::updates::activity::lease()?;
    let capture_activity = activity.clone();
    super::blocking::run_admitted(activity, move || {
        if !window.is_focused().unwrap_or(false) {
            return Err(AppError::Busy(
                "Откройте окно настроек для выбора кнопки.".into(),
            ));
        }
        crate::application::hotkeys::capture::begin(app, id, capture_activity)
    })
    .await
}

#[tauri::command]
pub async fn end_trigger_capture(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Выбор кнопки доступен в настройках Fono.".into(),
        ));
    }
    super::blocking::run(move || crate::application::hotkeys::capture::end(&app, &id)).await
}
