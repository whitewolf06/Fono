//! Speech model readiness and selection; downloads live in bounded adapters.
mod download;
mod wake_download;
mod whisper_download;
use crate::{
    error::{AppError, AppResult},
    state::{self, AppState},
    types::{WhisperModelInfo, WhisperModelSize},
};
pub use download::{cancel_download, has_active_downloads};
use std::path::Path;
use tauri::{AppHandle, Manager};
pub use wake_download::{download_kws_model, is_kws_model_downloaded};
pub use whisper_download::download_whisper_model;
/// Starts a best-effort background preload of the model selected in persisted
/// settings.  It deliberately does not make application startup fail: the
/// typed STT readiness IPC remains the source of truth for a missing model or
/// unavailable worker, and dictation still uses the same load gate.
pub fn preload_configured_stt(app: AppHandle) {
    let Ok(activity) = crate::application::updates::activity::lease() else {
        return;
    };
    let settings = app.state::<AppState>().settings();
    let Some((model_path, acceleration)) = configured_stt_preload(&settings) else {
        return;
    };
    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    let worker_paths = crate::stt::worker_paths_for_app(&app);

    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _activity = activity;
            stt.ensure_loaded(Path::new(&model_path), acceleration, &worker_paths)
        })
        .await;
        match result {
            Ok(Ok(())) => tracing::info!("configured STT model is ready after background preload"),
            Ok(Err(error)) => tracing::warn!(%error, "background STT preload failed"),
            Err(error) => tracing::warn!(%error, "background STT preload task failed"),
        }
    });
}

fn configured_stt_preload(
    settings: &crate::types::Settings,
) -> Option<(std::path::PathBuf, crate::types::AccelerationMode)> {
    settings
        .whisper_model_path
        .as_deref()
        .map(|path| (std::path::PathBuf::from(path), settings.acceleration))
}

pub fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    let dir = state::models_dir()?;
    let mut out = Vec::new();
    for size in WhisperModelSize::ALL {
        let filename = size.filename();
        let path = dir.join(filename);
        let (local_path, bytes) = if path.exists() {
            let meta = std::fs::metadata(&path).ok();
            (
                Some(path.to_string_lossy().to_string()),
                meta.map(|item| item.len()),
            )
        } else {
            (None, Some(size.approx_bytes()))
        };
        out.push(WhisperModelInfo {
            filename: filename.to_string(),
            size,
            local_path,
            bytes,
        });
    }
    Ok(out)
}

/// Makes a downloaded model active only after the STT runtime has accepted it.
/// This keeps persisted settings, the running engine and renderer events in
/// sync, so a successful "Choose" action never leaves a draft-only selection.
pub async fn set_whisper_model(app: &AppHandle, state: &AppState, path: String) -> AppResult<()> {
    let _activity = crate::application::updates::activity::lease()?;
    let _settings_transaction = crate::application::updates::settings_transaction(app).await;
    let model_path = std::path::PathBuf::from(&path);
    if path.trim().is_empty() || !model_path.is_file() {
        return Err(AppError::Config(format!(
            "файл модели не найден: {}",
            model_path.display()
        )));
    }

    let base = state.settings();
    let mut settings = base.clone();
    if settings.whisper_model_path.as_deref() == Some(path.as_str()) {
        return Ok(());
    }

    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    let acceleration = settings.acceleration;
    let worker_paths = crate::stt::worker_paths_for_app(app);
    let background_activity = _activity.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let _activity = background_activity;
        stt.ensure_loaded(&model_path, acceleration, &worker_paths)
    })
    .await
    .map_err(|error| AppError::Stt(format!("model preparation join failed: {error}")))?;
    prepared?;

    settings.whisper_model_path = Some(path);
    let settings = state.persist_settings_delta(&base, &settings)?;
    crate::events::emit_settings(app, &settings);
    tracing::info!(model = ?settings.whisper_model_path, "STT model selected and saved");
    Ok(())
}
