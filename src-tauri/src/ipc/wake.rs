//! IPC commands for controlling and validating wake-word detection.

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_wake_word_status(wake_handle: State<'_, fono_wake::WakeWordHandle>) -> String {
    wake_handle.status().to_string()
}

#[tauri::command]
pub fn get_wake_word_capabilities(
    wake_handle: State<'_, fono_wake::WakeWordHandle>,
) -> fono_wake::WakeWordCapabilities {
    wake_handle.capabilities()
}

#[tauri::command]
pub fn get_wake_word_diagnostics(
    wake_handle: State<'_, fono_wake::WakeWordHandle>,
) -> Option<fono_wake::Diagnostics> {
    wake_handle.diagnostics()
}

#[derive(Debug, serde::Serialize)]
pub struct WakeWordTestReport {
    pub detected: bool,
    pub keyword: String,
    pub json: String,
    pub duration_ms: u64,
}

#[cfg(feature = "sherpa-wake")]
#[tauri::command]
pub async fn test_wake_word_model(app: AppHandle) -> AppResult<WakeWordTestReport> {
    let activity = crate::application::updates::activity::lease()?;
    crate::application::dictation::ensure_capture_allowed(&app)?;
    let settings = app.state::<AppState>().settings();
    let mut config = crate::settings_to_wake_config(&settings)?;
    let (wav_path, builtin) = match config.backend {
        fono_wake::WakeWordBackend::SherpaStreamingRu => {
            config.phrase = "приедет бригада".into();
            (config.model_dir.join("0.wav"), false)
        }
        fono_wake::WakeWordBackend::SherpaStreamingEn => {
            config.phrase = "light up".into();
            (config.model_dir.join("test_wavs").join("0.wav"), false)
        }
        fono_wake::WakeWordBackend::SherpaOnnx => {
            (config.model_dir.join("test_wavs").join("0.wav"), true)
        }
        _ => {
            return Err(AppError::Config(
                "WAV-проверка доступна для Sherpa-движков".into(),
            ))
        }
    };
    if !wav_path.is_file() {
        return Err(AppError::Internal(format!(
            "тестовый WAV не найден: {}",
            wav_path.display()
        )));
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        fono_wake::test_with_wav(&config, &wav_path, builtin)
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake model test failed: {error}")))??;
    Ok(WakeWordTestReport {
        detected: result.detected,
        keyword: result.keyword,
        json: result.json,
        duration_ms: result.duration_ms,
    })
}

#[cfg(not(feature = "sherpa-wake"))]
#[tauri::command]
pub async fn test_wake_word_model(_app: AppHandle) -> AppResult<WakeWordTestReport> {
    Err(AppError::Internal(
        "sherpa-wake backend не собран в эту сборку".into(),
    ))
}

#[tauri::command]
pub async fn enable_wake_word(app: AppHandle) -> AppResult<()> {
    let _activity = crate::application::updates::activity::lease()?;
    let _settings_transaction = crate::application::updates::settings_transaction(&app).await;
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    let settings = state.settings();
    crate::application::wake_validation::ensure_profile_can_activate(&settings)?;
    if settings.wake_word_enabled
        && !matches!(
            wake_handle.status(),
            fono_wake::WakeWordStatus::Off
                | fono_wake::WakeWordStatus::Error
                | fono_wake::WakeWordStatus::MissingModel
        )
    {
        return Ok(());
    }
    let app_clone = app.clone();
    wake_handle.set_callback(move |event| match event {
        fono_wake::WakeWordEvent::Detected {
            phrase,
            pre_roll,
            audio_cursor,
        } => {
            tracing::info!("wake word triggered: {phrase}");
            crate::events::emit_wake_detected(&app_clone, &phrase);
            let handle = app_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    crate::application::wake_dictation::run(&handle, pre_roll, audio_cursor).await
                {
                    crate::application::dictation::resume_wake_if_idle(&handle);
                    tracing::error!("dictation after wake failed: {error}");
                    crate::events::emit_error(
                        &handle,
                        crate::events::ErrorCodeV1::Wake,
                        error.to_string(),
                        None,
                    );
                }
            });
        }
        fono_wake::WakeWordEvent::Error { message } => {
            crate::events::emit_error(&app_clone, crate::events::ErrorCodeV1::Wake, &message, None);
        }
        fono_wake::WakeWordEvent::Listening => {
            crate::events::emit_wake_status(&app_clone, crate::events::WakeStatusV1::Listening);
        }
        fono_wake::WakeWordEvent::Paused => {
            crate::events::emit_wake_status(&app_clone, crate::events::WakeStatusV1::Paused);
        }
        fono_wake::WakeWordEvent::ModelLoading => {
            crate::events::emit_wake_status(&app_clone, crate::events::WakeStatusV1::Loading);
        }
        fono_wake::WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            crate::events::emit_wake_status(&app_clone, crate::events::WakeStatusV1::MissingModel);
            crate::events::emit_error(
                &app_clone,
                crate::events::ErrorCodeV1::Wake,
                format!("Wake word: модель не найдена: {path}"),
                None,
            );
        }
    });

    let mut config = crate::settings_to_wake_config(&settings)?;
    config.enabled = true;
    wake_handle.update_config(config)?;

    let mut updated = settings.clone();
    updated.wake_word_enabled = true;
    let updated = match state.persist_settings_delta(&settings, &updated) {
        Ok(updated) => updated,
        Err(error) => {
            if let Ok(config) = crate::settings_to_wake_config(&state.settings()) {
                let _ = wake_handle.update_config(config);
            }
            return Err(error);
        }
    };
    crate::events::emit_settings(&app, &updated);
    tracing::info!("wake word enabled");
    Ok(())
}

#[tauri::command]
pub async fn disable_wake_word(app: AppHandle) -> AppResult<()> {
    let _activity = crate::application::updates::activity::lease()?;
    let _settings_transaction = crate::application::updates::settings_transaction(&app).await;
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    let previous = state.settings();
    wake_handle.stop();

    let mut updated = previous.clone();
    updated.wake_word_enabled = false;
    let updated = match state.persist_settings_delta(&previous, &updated) {
        Ok(updated) => updated,
        Err(error) => {
            if let Ok(config) = crate::settings_to_wake_config(&state.settings()) {
                let _ = wake_handle.update_config(config);
            }
            return Err(error);
        }
    };
    crate::events::emit_settings(&app, &updated);
    tracing::info!("wake word disabled");
    Ok(())
}

#[tauri::command]
pub async fn record_wake_word_sample(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<crate::application::wake::WakeWordSampleReport> {
    crate::application::wake::record_sample(app, duration_ms).await
}

#[tauri::command]
pub async fn recognize_wake_word_sample(
    app: AppHandle,
) -> AppResult<crate::application::wake::WakeWordRecognitionReport> {
    crate::application::wake::recognize_sample(app).await
}

#[tauri::command]
pub fn get_wake_calibration_status(
    app: AppHandle,
) -> crate::application::wake_calibration::WakeCalibrationStatus {
    crate::application::wake_calibration::status(&app)
}

#[tauri::command]
pub fn start_wake_calibration(
    app: AppHandle,
) -> AppResult<crate::application::wake_calibration::WakeCalibrationStatus> {
    crate::application::wake_calibration::start(&app)
}

#[tauri::command]
pub async fn record_wake_calibration_sample(
    app: AppHandle,
) -> AppResult<crate::application::wake_calibration::WakeCalibrationStatus> {
    crate::application::wake_calibration::record_next(app).await
}

#[tauri::command]
pub fn cancel_wake_calibration(
    app: AppHandle,
) -> AppResult<crate::application::wake_calibration::WakeCalibrationStatus> {
    crate::application::wake_calibration::cancel(&app)
}

#[tauri::command]
pub fn get_wake_profile_validation_status(
    app: AppHandle,
) -> crate::application::wake_validation::WakeProfileValidationStatus {
    crate::application::wake_validation::status(&app)
}

#[tauri::command]
pub fn start_wake_profile_validation(
    app: AppHandle,
) -> AppResult<crate::application::wake_validation::WakeProfileValidationStatus> {
    crate::application::wake_validation::start(&app)
}

#[tauri::command]
pub async fn record_wake_profile_validation_sample(
    app: AppHandle,
    kind: crate::application::wake_validation::WakeProfileValidationKind,
) -> AppResult<crate::application::wake_validation::WakeProfileValidationStatus> {
    crate::application::wake_validation::record(app, kind).await
}
