//! IPC commands that atomically apply persisted settings and their runtime effects.

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::stt::{SttHealth, SttReadiness};
use crate::types::{AccelerationCapabilities, Settings};

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    let mut settings = state.settings();
    settings.llm_api_key = None;
    settings
}

#[tauri::command]
pub fn get_acceleration_capabilities(app: AppHandle) -> AccelerationCapabilities {
    let (cuda, vulkan) = crate::stt::worker_paths_for_app(&app).capabilities();
    AccelerationCapabilities { cuda, vulkan }
}

#[tauri::command]
pub fn get_stt_readiness(app: AppHandle) -> SttReadiness {
    app.state::<crate::pipeline::Pipeline>().stt().readiness()
}

#[tauri::command]
pub fn get_stt_health(app: AppHandle) -> SttHealth {
    app.state::<crate::pipeline::Pipeline>().stt().health()
}

#[tauri::command]
pub async fn save_settings(
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
    let model_changed = old_settings.whisper_model_path != settings.whisper_model_path
        || old_settings.acceleration != settings.acceleration;

    if shortcuts_changed {
        if let Err(error) = crate::register_all_shortcuts(&app, &settings) {
            let rollback = restore_shortcuts(&app, &old_settings);
            return Err(AppError::Config(format!(
                "Не удалось зарегистрировать горячие клавиши: {error}. {rollback}"
            )));
        }
    }

    let previous_secret = crate::secrets::load_llm_api_key()?;
    let persisted_settings = match prepare_secret_update(settings) {
        Ok(settings) => settings,
        Err(error) => {
            if shortcuts_changed {
                let _ = restore_shortcuts(&app, &old_settings);
            }
            return Err(error);
        }
    };

    if model_changed {
        if let Some(path) = persisted_settings.whisper_model_path.clone() {
            let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
            let acceleration = persisted_settings.acceleration;
            let worker_paths = crate::stt::worker_paths_for_app(&app);
            let prepared = match tauri::async_runtime::spawn_blocking(move || {
                stt.ensure_loaded(std::path::Path::new(&path), acceleration, &worker_paths)
            })
            .await
            {
                Ok(result) => result,
                Err(error) => Err(AppError::Stt(format!(
                    "model preparation join failed: {error}"
                ))),
            };
            if let Err(error) = prepared {
                let _ = restore_secret(previous_secret.as_deref());
                if shortcuts_changed {
                    let _ = restore_shortcuts(&app, &old_settings);
                }
                return Err(error);
            }
        }
    }

    let wake_reconfigured = persisted_settings.wake_word_enabled && wake_settings_changed;
    if wake_reconfigured {
        let config = match crate::settings_to_wake_config(&persisted_settings) {
            Ok(config) => config,
            Err(error) => {
                let _ = restore_secret(previous_secret.as_deref());
                if shortcuts_changed {
                    let _ = restore_shortcuts(&app, &old_settings);
                }
                return Err(error);
            }
        };
        if let Err(error) = app
            .state::<fono_wake::WakeWordHandle>()
            .update_config(config)
        {
            let _ = restore_secret(previous_secret.as_deref());
            if shortcuts_changed {
                let _ = restore_shortcuts(&app, &old_settings);
            }
            return Err(AppError::Config(format!(
                "Не удалось подготовить новые настройки wake word: {error}"
            )));
        }
    }

    if let Err(error) = state::save_settings(&persisted_settings) {
        let secret_rollback = restore_secret(previous_secret.as_deref());
        if wake_reconfigured {
            if let Ok(config) = crate::settings_to_wake_config(&old_settings) {
                let _ = app
                    .state::<fono_wake::WakeWordHandle>()
                    .update_config(config);
            }
        }
        if shortcuts_changed {
            let rollback = restore_shortcuts(&app, &old_settings);
            return Err(AppError::Config(format!(
                "Не удалось сохранить новые настройки: {error}. {rollback} {secret_rollback}"
            )));
        }
        return Err(AppError::Config(format!(
            "Не удалось сохранить новые настройки: {error}. {secret_rollback}"
        )));
    }

    if let Err(error) = crate::history::apply_analytics_privacy_policy(
        persisted_settings.analytics_enabled && persisted_settings.history_enabled,
        persisted_settings.analytics_retention_days,
    ) {
        tracing::error!(%error, "could not apply dictation analytics privacy policy");
        return Err(AppError::Config(format!(
            "Настройки сохранены, но не удалось применить правила хранения аналитики: {error}"
        )));
    }

    state.set_settings(persisted_settings.clone());
    crate::verbose::set_verbose(persisted_settings.verbose_logging);
    crate::events::emit_settings(&app, &persisted_settings);
    tracing::info!(
        "settings saved: model={:?}, lang={}",
        persisted_settings.whisper_model_path,
        persisted_settings.language
    );

    Ok(())
}

fn prepare_secret_update(mut settings: Settings) -> AppResult<Settings> {
    let submitted_secret = settings
        .llm_api_key
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    match submitted_secret {
        Some(value) => {
            crate::secrets::store_llm_api_key(&value)?;
            settings.has_llm_api_key = true;
        }
        None if settings.has_llm_api_key => {
            settings.has_llm_api_key = crate::secrets::load_llm_api_key()?.is_some();
        }
        None => {
            crate::secrets::delete_llm_api_key()?;
            settings.has_llm_api_key = false;
        }
    }
    Ok(settings)
}

fn restore_secret(previous: Option<&str>) -> String {
    let result = match previous {
        Some(value) => crate::secrets::store_llm_api_key(value),
        None => crate::secrets::delete_llm_api_key(),
    };
    match result {
        Ok(()) => "Предыдущий API key восстановлен.".into(),
        Err(error) => {
            tracing::error!(%error, "failed to roll back LLM API key");
            format!("Не удалось восстановить предыдущий API key: {error}")
        }
    }
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
    if !(1..=365).contains(&settings.analytics_retention_days) {
        return Err(AppError::Config(
            "Срок хранения аналитики должен быть от 1 до 365 дней".into(),
        ));
    }
    let wake_config = crate::settings_to_wake_config(settings)?;
    fono_wake::validate_config(&wake_config)
        .map_err(|error| AppError::Config(format!("Некорректная wake phrase: {error}")))?;
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
    fn settings_validation_rejects_unsupported_sherpa_phrase() {
        let settings = Settings {
            wake_backend: crate::types::WakeWordBackend::SherpaOnnx,
            wake_word: "привет фоно".into(),
            ..Settings::default()
        };

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
