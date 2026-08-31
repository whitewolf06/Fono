//! IPC commands that atomically apply persisted settings and their runtime effects.

use std::collections::BTreeMap;

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::stt::{SttHealth, SttReadiness};
use crate::types::{AccelerationCapabilities, Settings};

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    let mut settings = state.settings();
    settings.llm_api_key = None;
    for profile in &mut settings.llm_profiles {
        profile.api_key = None;
    }
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
    mut settings: Settings,
) -> AppResult<()> {
    let old_settings = state.settings();
    normalize_llm_profiles(&mut settings, &old_settings);
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

    let previous_secrets = profile_secret_snapshot(&old_settings, &settings)?;
    let persisted_settings = match prepare_secret_update(settings, &old_settings) {
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
                let _ = restore_profile_secrets(&previous_secrets);
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
                let _ = restore_profile_secrets(&previous_secrets);
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
            let _ = restore_profile_secrets(&previous_secrets);
            if shortcuts_changed {
                let _ = restore_shortcuts(&app, &old_settings);
            }
            return Err(AppError::Config(format!(
                "Не удалось подготовить новые настройки wake word: {error}"
            )));
        }
    }

    if let Err(error) = state::save_settings(&persisted_settings) {
        let secret_rollback = restore_profile_secrets(&previous_secrets);
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

fn prepare_secret_update(mut settings: Settings, old_settings: &Settings) -> AppResult<Settings> {
    normalize_llm_profiles(&mut settings, old_settings);

    let submitted_secret = settings
        .llm_api_key
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(value) = submitted_secret {
        if let Some(default) = settings
            .llm_profiles
            .iter_mut()
            .find(|profile| profile.id == crate::types::LlmProfile::DEFAULT_ID)
        {
            default.api_key = Some(value);
            default.has_api_key = true;
        }
    }

    for profile in &mut settings.llm_profiles {
        let submitted_secret = profile
            .api_key
            .take()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        match submitted_secret {
            Some(value) => {
                crate::secrets::store_llm_profile_api_key(&profile.id, &value)?;
                profile.has_api_key = true;
            }
            None if profile.has_api_key => {
                profile.has_api_key =
                    crate::secrets::load_llm_profile_api_key(&profile.id)?.is_some();
            }
            None => {
                crate::secrets::delete_llm_profile_api_key(&profile.id)?;
                profile.has_api_key = false;
            }
        }
    }
    mirror_default_profile_to_legacy(&mut settings);
    Ok(settings)
}

fn normalize_llm_profiles(settings: &mut Settings, old_settings: &Settings) {
    // Existing UI clients send only the legacy fields. Keep all named profiles
    // intact and mirror those fields into the default profile for compatibility.
    if settings.llm_profiles.is_empty() {
        settings.llm_profiles = old_settings.llm_profiles.clone();
        settings.migrate_llm_profiles();
        let provider = settings.llm_provider;
        let base_url = settings.llm_base_url.clone();
        let model = settings.llm_model.clone();
        let has_api_key = settings.has_llm_api_key;
        let connection = if matches!(provider, crate::types::LlmProvider::OpenAi) {
            crate::types::LlmConnectionKind::Cloud
        } else {
            crate::types::LlmConnectionKind::Local
        };
        if let Some(default) = settings
            .llm_profiles
            .iter_mut()
            .find(|profile| profile.id == crate::types::LlmProfile::DEFAULT_ID)
        {
            default.provider = provider;
            default.base_url = base_url;
            default.model = model;
            default.connection = connection;
            default.has_api_key = has_api_key;
        }
    } else {
        settings.migrate_llm_profiles();
    }
}

fn mirror_default_profile_to_legacy(settings: &mut Settings) {
    let Some(profile) = settings
        .llm_profiles
        .iter()
        .find(|profile| profile.id == crate::types::LlmProfile::DEFAULT_ID)
    else {
        return;
    };
    settings.llm_provider = profile.provider;
    settings.llm_base_url = profile.base_url.clone();
    settings.llm_model = profile.model.clone();
    settings.has_llm_api_key = profile.has_api_key;
}

fn profile_secret_snapshot(
    old_settings: &Settings,
    submitted_settings: &Settings,
) -> AppResult<BTreeMap<String, Option<String>>> {
    let ids = old_settings
        .llm_profiles
        .iter()
        .chain(submitted_settings.llm_profiles.iter())
        .map(|profile| profile.id.as_str())
        .chain(std::iter::once(crate::types::LlmProfile::DEFAULT_ID));
    let mut snapshot = BTreeMap::new();
    for id in ids {
        if !snapshot.contains_key(id) {
            snapshot.insert(id.to_owned(), crate::secrets::load_llm_profile_api_key(id)?);
        }
    }
    Ok(snapshot)
}

fn restore_profile_secrets(previous: &BTreeMap<String, Option<String>>) -> String {
    let failed = previous.iter().find_map(|(id, value)| {
        let result = match value {
            Some(value) => crate::secrets::store_llm_profile_api_key(id, value),
            None => crate::secrets::delete_llm_profile_api_key(id),
        };
        result.err().map(|error| (id, error))
    });
    match failed {
        None => "Предыдущие API keys восстановлены.".into(),
        Some((id, error)) => {
            tracing::error!(%error, profile_id = id, "failed to roll back LLM API key");
            format!("Не удалось восстановить API key профиля {id}: {error}")
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
    let mut ids = std::collections::BTreeSet::new();
    for profile in &settings.llm_profiles {
        let valid_id = !profile.id.is_empty()
            && profile.id.len() <= 64
            && profile
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !valid_id || !ids.insert(profile.id.as_str()) || profile.name.trim().is_empty() {
            return Err(AppError::Config(
                "Каждому LLM-профилю нужны уникальный id и название".into(),
            ));
        }
        if profile.base_url.trim().is_empty() {
            return Err(AppError::Config(
                "URL LLM-профиля не может быть пустым".into(),
            ));
        }
    }
    for assignment in [
        settings.text_correction_llm.profile_id.as_deref(),
        settings.speech_analysis_llm.profile_id.as_deref(),
    ] {
        if let Some(id) = assignment {
            if !ids.contains(id) {
                return Err(AppError::Config("Выбран неизвестный LLM-профиль".into()));
            }
        }
    }
    if settings.speech_analysis_llm.enabled && settings.speech_analysis_llm.profile_id.is_none() {
        return Err(AppError::Config(
            "Для LLM-анализа речи выберите профиль".into(),
        ));
    }
    if let Some(profile) = settings.speech_analysis_profile() {
        if profile.connection == crate::types::LlmConnectionKind::Cloud
            && settings.speech_analysis_llm.enabled
            && settings.speech_analysis_llm.data_scope
                != crate::types::SpeechLlmDataScope::MetricsOnly
            && !settings.speech_analysis_llm.cloud_consent
        {
            return Err(AppError::Config(
                "Для отправки фрагментов или текста в облачный LLM требуется согласие".into(),
            ));
        }
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
