//! IPC commands that atomically apply persisted settings and their runtime effects.

use std::collections::BTreeMap;

use tauri::{AppHandle, Emitter, Manager, State};
mod wake;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::stt::{SttHealth, SttReadiness};
use crate::types::{AccelerationCapabilities, Settings};
mod active;

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
    let _activity = crate::application::updates::activity::lease()?;
    let _transaction = crate::application::updates::settings_transaction(&app).await;
    settings.enforce_classic_dictation();
    settings.enforce_wake_availability();
    settings.migrate_processing_prompts();
    let old_settings = state.settings();
    // Update checks are owned by their explicit opt-in command. A stale full
    // settings form, including an older renderer, cannot silently change it.
    settings.update_checks_enabled = old_settings.update_checks_enabled;
    let _configuration = crate::application::capture_configuration::reserve_configuration(|| {
        if crate::application::dictation::workflow::is_busy(&app) {
            return Err(AppError::Busy(
                "Сначала обработайте, вставьте или отмените ожидающую диктовку".into(),
            ));
        }
        active::validate(
            app.state::<crate::pipeline::Pipeline>()
                .current_operation()
                .is_some(),
            &old_settings,
            &settings,
        )
    })?;
    let wake_settings_changed = wake::invalidate_changed(&mut settings, &old_settings);
    normalize_llm_profiles(&mut settings, &old_settings);
    validate_settings(&settings)?;
    if settings.wake_word_enabled {
        crate::application::wake_validation::ensure_profile_can_activate(&settings)?;
    }
    let shortcuts_changed = old_settings.hotkey != settings.hotkey
        || old_settings.hotkey_mode != settings.hotkey_mode
        || old_settings.command_hotkey != settings.command_hotkey;
    let model_changed = old_settings.whisper_model_path != settings.whisper_model_path
        || old_settings.acceleration != settings.acceleration;

    let previous_secrets = profile_secret_snapshot(&old_settings, &settings)?;
    let persisted_settings = match prepare_secret_update(settings, &old_settings) {
        Ok(settings) => settings,
        Err(error) => {
            let _ = restore_profile_secrets(&previous_secrets);
            return Err(error);
        }
    };

    if model_changed {
        if let Some(path) = persisted_settings.whisper_model_path.clone() {
            let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
            let acceleration = persisted_settings.acceleration;
            let worker_paths = crate::stt::worker_paths_for_app(&app);
            let background_activity = _activity.clone();
            let background_configuration = _configuration.clone();
            let prepared = match tauri::async_runtime::spawn_blocking(move || {
                let _activity = background_activity;
                let _configuration = background_configuration;
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
                return Err(error);
            }
        }
    }

    // No awaits remain after registration: cancellation during model
    // preparation cannot leave new shortcuts paired with old persisted values.
    if shortcuts_changed {
        if let Err(error) = crate::register_all_shortcuts(&app, &persisted_settings) {
            let rollback = restore_shortcuts(&app, &old_settings);
            let _ = restore_profile_secrets(&previous_secrets);
            return Err(AppError::Config(format!(
                "Не удалось зарегистрировать горячие клавиши: {error}. {rollback}"
            )));
        }
    }

    let wake_reconfigured = old_settings.wake_word_enabled != persisted_settings.wake_word_enabled
        || (persisted_settings.wake_word_enabled && wake_settings_changed);
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

    let autostart_changed = old_settings.autostart != persisted_settings.autostart;
    let persist_result = if autostart_changed {
        crate::autostart::set_enabled(persisted_settings.autostart)
    } else {
        Ok(())
    }
    .and_then(|_| state.persist_settings_delta(&old_settings, &persisted_settings));
    let persisted_settings = match persist_result {
        Ok(settings) => settings,
        Err(error) => {
            if autostart_changed {
                let _ = crate::autostart::set_enabled(old_settings.autostart);
            }
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
    };

    if let Err(error) = delete_removed_profile_secrets(&old_settings, &persisted_settings) {
        tracing::error!(%error, "could not delete removed LLM profile credentials");
        return Err(AppError::Config(format!(
            "Настройки сохранены, но не удалось удалить ключ удалённого LLM-профиля: {error}"
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

    crate::verbose::set_verbose(persisted_settings.verbose_logging);
    crate::events::emit_settings(&app, &persisted_settings);
    if wake_settings_changed {
        crate::events::emit_wake_status(&app, crate::events::WakeStatusV1::Paused);
        let _=app.emit("wake-word-configuration-invalidated",wake::WakeConfigurationInvalidated{
            reason:"configuration_changed",
            message:"Настройки пробуждения изменились. Фраза выключена: настройте и проверьте её заново.",
        });
    }
    crate::pipeline::sync_overlay_window(&app, state.pipeline_state());
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

fn delete_removed_profile_secrets(
    old_settings: &Settings,
    persisted_settings: &Settings,
) -> AppResult<()> {
    for profile in &old_settings.llm_profiles {
        let still_exists = persisted_settings
            .llm_profiles
            .iter()
            .any(|candidate| candidate.id == profile.id);
        if !still_exists {
            crate::secrets::delete_llm_profile_api_key(&profile.id)?;
        }
    }
    Ok(())
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
pub fn save_overlay_position(app: AppHandle, x: i32, y: i32) -> AppResult<()> {
    crate::overlay::save_position(&app, x, y)
}

fn validate_settings(settings: &Settings) -> AppResult<()> {
    settings.processing_prompts.validate()?;
    crate::application::personal_dictionary::validate(&settings.personal_dictionary_entries)?;
    crate::overlay::validate_appearance(settings.overlay_scale, settings.overlay_opacity)?;
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
    for id in [
        settings.text_correction_llm.profile_id.as_deref(),
        settings.speech_analysis_llm.profile_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !ids.contains(id) {
            return Err(AppError::Config("Выбран неизвестный LLM-профиль".into()));
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
    fn settings_validation_accepts_ramzi_sherpa_phrase() {
        let settings = Settings {
            wake_backend: crate::types::WakeWordBackend::SherpaOnnx,
            wake_word: "рамзи".into(),
            ..Settings::default()
        };

        assert!(validate_settings(&settings).is_ok());
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
