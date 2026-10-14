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

fn require_feedback_window(label: &str) -> AppResult<()> {
    if label != "settings" {
        return Err(AppError::Config(
            "Форму обратной связи можно открыть из основного окна Fono.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn open_feedback_form(window: WebviewWindow, include_context: bool) -> AppResult<()> {
    require_feedback_window(window.label())?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::application::feedback_form::open(include_context)
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось открыть браузер.".into()))?
}

#[cfg(test)]
mod tests {
    use super::require_feedback_window;

    #[test]
    fn feedback_form_only_allows_settings_window() {
        assert!(require_feedback_window("settings").is_ok());
        for label in ["overlay", "main", "external", ""] {
            assert!(require_feedback_window(label).is_err());
        }
    }
}
