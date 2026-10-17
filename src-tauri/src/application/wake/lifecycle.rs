//! Frozen wake-word service lifecycle; compiled only with `wake-word`.

use crate::types::Settings;
use crate::{events, pipeline, state, stt, types};
use fono_wake::{WakeWordConfig, WakeWordEvent, WakeWordHandle};
use tauri::Manager;

/// Builds a backend-specific wake word config from application settings.
#[cfg(feature = "wake-word")]
pub(crate) fn settings_to_wake_config(
    settings: &Settings,
) -> crate::error::AppResult<WakeWordConfig> {
    let model_dir = fono_wake::model_spec(settings.wake_backend)
        .map(|spec| state::app_data_dir().map(|dir| dir.join("kws-models").join(spec.directory)))
        .transpose()?
        .unwrap_or_default();
    Ok(WakeWordConfig {
        enabled: types::WAKE_WORD_AVAILABLE && settings.wake_word_enabled,
        backend: settings.wake_backend,
        phrase: settings.wake_word.clone(),
        audio_device_id: settings.audio_device_id.clone(),
        sample_rate: 16_000,
        threshold: settings.wake_word_threshold,
        sensitivity: settings.wake_word_sensitivity,
        vad_threshold: settings.wake_word_vad_threshold,
        use_gpu: settings.acceleration.use_gpu(),
        cooldown_ms: 2_000,
        phrase_stability_ms: 150 + (settings.wake_word_threshold.clamp(0.0, 1.0) * 700.0) as u64,
        phrase_confirmations: 2,
        model_dir,
    })
}

/// Запускает wake word детектор, если он включён в настройках.
///
/// Использует отдельно выбранный исследовательский streaming backend.
/// При обнаружении wake-фразы стартует запись диктовки,
/// по тишине (VAD) — STT (основной моделью) + вставка текста.
#[cfg(feature = "wake-word")]
pub(crate) async fn start_if_enabled(
    handle: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    if !types::WAKE_WORD_AVAILABLE {
        tracing::info!("wake word: temporarily unavailable, skipping");
        return Ok(());
    }
    let activity = crate::application::updates::activity::lease()?;
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let settings = state.settings();

    if !settings.wake_word_enabled {
        tracing::info!("wake word: disabled in settings, skipping");
        return Ok(());
    }
    crate::application::wake_validation::ensure_profile_can_activate(&settings)?;

    let wake_handle = handle.state::<WakeWordHandle>();
    let handle_clone = handle.clone();
    wake_handle.set_callback(move |event| match event {
        WakeWordEvent::Detected {
            phrase,
            pre_roll,
            audio_cursor,
        } => {
            tracing::info!("wake word detected: {phrase}");
            events::emit_wake_detected(&handle_clone, &phrase);
            let h = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    crate::application::wake_dictation::run(&h, pre_roll, audio_cursor).await
                {
                    crate::application::dictation::resume_wake_if_idle(&h);
                    tracing::error!("dictation after wake failed: {e:?}");
                    events::emit_error(&h, events::ErrorCodeV1::Wake, e.to_string(), None);
                }
            });
        }
        WakeWordEvent::Error { message } => {
            tracing::error!("wake word error: {message}");
            events::emit_error(&handle_clone, events::ErrorCodeV1::Wake, &message, None);
        }
        WakeWordEvent::Listening => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Listening);
        }
        WakeWordEvent::Paused => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Paused);
        }
        WakeWordEvent::ModelLoading => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Loading);
        }
        WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::MissingModel);
            events::emit_error(
                &handle_clone,
                events::ErrorCodeV1::Wake,
                format!("Wake word: модель не найдена. Скачайте её в настройках: {path}"),
                None,
            );
        }
    });

    // Подготавливаем основную модель до того, как detector начнёт принимать
    // ключевую фразу. Раньше первая диктовка после запуска попадала в STT без
    // контекста модели и оставляла wake word на паузе.
    if let Some(path) = settings.whisper_model_path.clone() {
        let stt = handle.state::<pipeline::Pipeline>().stt().clone();
        let acceleration = settings.acceleration;
        let worker_paths = stt::worker_paths_for_app(handle);
        events::emit_wake_status(handle, events::WakeStatusV1::Loading);
        let background_activity = activity.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _activity = background_activity;
            stt.ensure_loaded(std::path::Path::new(&path), acceleration, &worker_paths)
        })
        .await
        .map_err(|e| format!("primary whisper load join: {e}"))??;
    } else {
        return Err("Whisper-модель для диктовки не выбрана".into());
    }

    let config = settings_to_wake_config(&settings)?;
    wake_handle.update_config(config)?;

    Ok(())
}

/// Перезапускает wake word детектор (без изменения флага wake_word_enabled).
/// Используется когда пользователь меняет настройки wake word "на лету".
#[cfg(feature = "wake-word")]
pub async fn restart(handle: &tauri::AppHandle) -> Result<(), String> {
    let wake_handle = handle.state::<WakeWordHandle>();
    let settings = handle.state::<state::AppState>().settings();
    let config = settings_to_wake_config(&settings).map_err(|e| e.to_string())?;
    wake_handle.update_config(config).map_err(|e| e.to_string())
}
