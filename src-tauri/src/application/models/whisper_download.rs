use super::{
    download::{download_client, emit_download, ensure_success, DownloadRegistration},
    store::install_verified_model,
};
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
    let filename = artifact.filename;
    let download_id = format!("whisper:{}", artifact.variant);
    let registration = DownloadRegistration::begin(download_id.clone())?;
    let url = artifact.url;
    let expected_sha256 = artifact.sha256;
    let expected_bytes = artifact.bytes;

    emit_download(
        &app,
        &download_id,
        &filename,
        ModelDownloadPhaseV1::Started,
        0,
        None,
    );
    let app_clone = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let result = (|| -> AppResult<()> {
            let target = state::models_dir()?.join(&filename);
            if target.exists() {
                return Err(AppError::Config(
                    "Model already exists; keeping existing file".into(),
                ));
            }
            let client = download_client()?;
            let mut response = client
                .get(&url)
                .send()
                .map_err(|error| AppError::Config(format!("GET {url}: {error}")))?;
            ensure_success(&response, "Whisper model")?;
            let total = response.content_length();
            let receipt = install_verified_model(
                &mut response,
                &target,
                expected_bytes,
                &expected_sha256,
                &registration.cancellation,
                |bytes| {
                    emit_download(
                        &app_clone,
                        &download_id,
                        &filename,
                        ModelDownloadPhaseV1::Downloading,
                        bytes,
                        total,
                    );
                },
                || {
                    emit_download(
                        &app_clone,
                        &download_id,
                        &filename,
                        ModelDownloadPhaseV1::Verifying,
                        expected_bytes,
                        Some(expected_bytes),
                    )
                },
            )?;
            emit_download(
                &app_clone,
                &download_id,
                &filename,
                ModelDownloadPhaseV1::Completed,
                receipt.bytes,
                total,
            );
            Ok(())
        })();
        if matches!(result, Err(AppError::Cancelled(_))) {
            emit_download(
                &app_clone,
                &download_id,
                &filename,
                ModelDownloadPhaseV1::Cancelled,
                0,
                None,
            );
        } else if let Err(error) = &result {
            emit_download(
                &app_clone,
                &download_id,
                &filename,
                ModelDownloadPhaseV1::Failed,
                0,
                None,
            );
            crate::events::emit_error(
                &app_clone,
                crate::events::ErrorCodeV1::Download,
                error.to_string(),
                None,
            );
        }
        result
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?;
    result
}
