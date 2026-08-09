//! IPC commands that atomically apply persisted settings and their runtime effects.

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::types::{AccelerationCapabilities, Settings};

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn get_acceleration_capabilities(app: AppHandle) -> AccelerationCapabilities {
    let (cuda, vulkan) = crate::stt::worker_paths_for_app(&app).capabilities();
    AccelerationCapabilities { cuda, vulkan }
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<()> {
    let old_settings = state.settings();
    validate_settings(&settings)?;
    let shortcuts_changed = old_settings.hotkey != settings.hotkey
        || old_settings.command_hotkey != settings.command_hotkey;
    let wake_settings_changed = old_settings.wake_word != settings.wake_word
        || old_settings.wake_word_model != settings.wake_word_model
        || old_settings.wake_backend != settings.wake_backend
        || (old_settings.wake_word_threshold - settings.wake_word_threshold).abs() > f32::EPSILON
        || (old_settings.wake_word_sensitivity - settings.wake_word_sensitivity).abs()
            > f32::EPSILON
        || (old_settings.wake_word_vad_threshold - settings.wake_word_vad_threshold).abs()
            > f32::EPSILON;

    if shortcuts_changed {
        if let Err(error) = crate::register_all_shortcuts(&app, &settings) {
            let rollback = restore_shortcuts(&app, &old_settings);
            return Err(AppError::Config(format!(
                "Не удалось зарегистрировать горячие клавиши: {error}. {rollback}"
            )));
        }
    }

    if let Err(error) = state::save_settings(&settings) {
        if shortcuts_changed {
            let rollback = restore_shortcuts(&app, &old_settings);
            return Err(AppError::Config(format!(
                "Не удалось сохранить новые настройки: {error}. {rollback}"
            )));
        }
        return Err(error);
    }

    state.set_settings(settings.clone());
    crate::verbose::set_verbose(settings.verbose_logging);
    let _ = app.emit("settings-changed", settings.clone());
    tracing::info!(
        "settings saved: model={:?}, lang={}",
        settings.whisper_model_path,
        settings.language
    );

    if settings.wake_word_enabled && wake_settings_changed {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = crate::restart_wake_word(&app_clone).await {
                tracing::error!("wake word restart after settings change failed: {error}");
                let _ = app_clone.emit(
                    "error",
                    format!("Wake word: не удалось перезапустить: {error}"),
                );
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub fn save_overlay_position(state: State<'_, AppState>, x: i32, y: i32) -> AppResult<()> {
    let mut settings = state.settings();
    settings.overlay_x = Some(x);
    settings.overlay_y = Some(y);
    state::save_settings(&settings)?;
    state.set_settings(settings);
    Ok(())
}

fn validate_settings(settings: &Settings) -> AppResult<()> {
    if settings.hotkey.trim().is_empty() || settings.command_hotkey.trim().is_empty() {
        return Err(AppError::Config(
            "Горячие клавиши не могут быть пустыми".into(),
        ));
    }
    if settings
        .hotkey
        .eq_ignore_ascii_case(&settings.command_hotkey)
    {
        return Err(AppError::Config(
            "Горячие клавиши диктовки и команд должны отличаться".into(),
        ));
    }
    if !settings.wake_word_threshold.is_finite()
        || !settings.wake_word_sensitivity.is_finite()
        || !settings.wake_word_vad_threshold.is_finite()
        || !settings.wake_dictation_speech_threshold.is_finite()
    {
        return Err(AppError::Config(
            "Параметры wake word должны быть конечными числами".into(),
        ));
    }
    Ok(())
}

fn restore_shortcuts(app: &AppHandle, settings: &Settings) -> String {
    match crate::register_all_shortcuts(app, settings) {
        Ok(()) => "Старые значения восстановлены.".to_string(),
        Err(error) => {
            tracing::error!("failed to restore previous shortcuts: {error}");
            format!("Не удалось восстановить старые горячие клавиши: {error}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validation_rejects_duplicate_hotkeys() {
        let mut settings = Settings::default();
        settings.command_hotkey = settings.hotkey.clone();

        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn settings_validation_rejects_non_finite_wake_values() {
        let settings = Settings {
            wake_word_threshold: f32::NAN,
            ..Settings::default()
        };

        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn settings_validation_accepts_defaults() {
        assert!(validate_settings(&Settings::default()).is_ok());
    }
}
