//! IPC commands for controlling and validating wake-word detection.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_wake_word_status(wake_handle: State<'_, fono_wake::WakeWordHandle>) -> String {
    wake_handle.status().to_string()
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
            let _ = app_clone.emit("wake-word-detected", &phrase);
            let handle = app_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = crate::run_dictation_after_wake(&handle, pre_roll).await {
                    tracing::error!("dictation after wake failed: {error}");
                    let _ = handle.emit("error", error.to_string());
                }
            });
        }
        fono_wake::WakeWordEvent::Error { message } => {
            let _ = app_clone.emit("error", &message);
        }
        fono_wake::WakeWordEvent::Listening => {
            let _ = app_clone.emit("wake-word-status", "listening");
        }
        fono_wake::WakeWordEvent::Paused => {
            let _ = app_clone.emit("wake-word-status", "paused");
        }
        fono_wake::WakeWordEvent::ModelLoading => {
            let _ = app_clone.emit("wake-word-status", "loading");
        }
        fono_wake::WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            let _ = app_clone.emit("wake-word-status", "missing_model");
            let _ = app_clone.emit("error", format!("Wake word: модель не найдена: {path}"));
        }
    });

    let mut config = crate::settings_to_wake_config(&settings)?;
    config.enabled = true;
    wake_handle.update_config(config)?;

    let mut updated = settings;
    updated.wake_word_enabled = true;
    crate::state::save_settings(&updated)?;
    state.set_settings(updated);
    tracing::info!("wake word enabled");
    Ok(())
}

#[tauri::command]
pub async fn disable_wake_word(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    app.state::<fono_wake::WakeWordHandle>().stop();

    let mut updated = state.settings();
    updated.wake_word_enabled = false;
    crate::state::save_settings(&updated)?;
    state.set_settings(updated);
    tracing::info!("wake word disabled");
    Ok(())
}
