//! Async desktop ownership and event adaptation for shared Whisper downloads.

use super::download::{emit_download, progress, DownloadRegistration};
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
    state,
    types::{WhisperArtifact, WhisperModelSize},
};
use tauri::AppHandle;

pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let model_size: WhisperModelSize =
        serde_json::from_value(serde_json::Value::String(size.clone()))
            .map_err(|_| AppError::Config(format!("неизвестный размер модели: {size}")))?;
    download_whisper_artifact(app, WhisperArtifact::from_size(model_size)).await
}

pub async fn download_whisper_artifact(app: AppHandle, artifact: WhisperArtifact) -> AppResult<()> {
    let download_id = format!("whisper:{}", artifact.variant);
    let registration = DownloadRegistration::begin(download_id.clone())?;
    emit_download(
        &app,
        &download_id,
        &artifact.filename,
        ModelDownloadPhaseV1::Started,
        0,
        None,
    );
    tauri::async_runtime::spawn_blocking(move || {
        let progress = progress(app.clone());
        let result = match state::models_dir() {
            Ok(directory) => fono_stt::models::whisper_download::download_whisper_artifact(
                &directory,
                artifact,
                &registration.cancellation,
                &progress,
            )
            .map_err(AppError::from),
            Err(error) => {
                emit_download(
                    &app,
                    &download_id,
                    &artifact.filename,
                    ModelDownloadPhaseV1::Failed,
                    0,
                    None,
                );
                Err(error)
            }
        };
        if let Err(error) = &result {
            if !matches!(error, AppError::Cancelled(_)) {
                crate::events::emit_error(
                    &app,
                    crate::events::ErrorCodeV1::Download,
                    error.to_string(),
                    None,
                );
            }
        }
        result
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}
