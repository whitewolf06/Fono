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
    let settings = app.state::<AppState>().settings();
    let config = crate::settings_to_wake_config(&settings)?;
    let wav_path = config.model_dir.join("test_wavs").join("0.wav");
    if !wav_path.is_file() {
        return Err(AppError::Internal(format!(
            "тестовый WAV не найден: {}",
            wav_path.display()
        )));
    }
    let result = fono_wake::test_with_wav(&config, &wav_path, true)?;
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
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    let settings = state.settings();
    let app_clone = app.clone();
    wake_handle.set_callback(move |event| match event {
        fono_wake::WakeWordEvent::Detected { phrase, pre_roll } => {
            tracing::info!("wake word triggered: {phrase}");
            crate::events::emit_wake_detected(&app_clone, &phrase);
            let handle = app_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = crate::run_dictation_after_wake(&handle, pre_roll).await {
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

    let mut updated = settings;
    updated.wake_word_enabled = true;
    if let Err(error) = crate::state::save_settings(&updated) {
        let mut rollback = updated.clone();
        rollback.wake_word_enabled = false;
        if let Ok(config) = crate::settings_to_wake_config(&rollback) {
            let _ = wake_handle.update_config(config);
        }
        return Err(error);
    }
    state.set_settings(updated);
    tracing::info!("wake word enabled");
    Ok(())
}

#[tauri::command]
pub async fn disable_wake_word(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    let previous = state.settings();
    wake_handle.stop();

    let mut updated = previous.clone();
    updated.wake_word_enabled = false;
    if let Err(error) = crate::state::save_settings(&updated) {
        if let Ok(config) = crate::settings_to_wake_config(&previous) {
            let _ = wake_handle.update_config(config);
        }
        return Err(error);
    }
    state.set_settings(updated);
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
