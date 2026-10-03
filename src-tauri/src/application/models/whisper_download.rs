use super::download::{
    copy_verified, download_client, emit_download, ensure_success, DownloadRegistration,
};
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
    state,
    types::WhisperModelSize,
};
use tauri::AppHandle;
pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let model_size: WhisperModelSize =
        serde_json::from_value(serde_json::Value::String(size.clone()))
            .map_err(|_| AppError::Config(format!("неизвестный размер модели: {size}")))?;
    let filename = model_size.filename().to_string();
    let download_id = format!("whisper:{size}");
    let registration = DownloadRegistration::begin(download_id.clone())?;
    let url = model_size.url().to_string();
    let expected_sha256 = model_size.sha256().to_string();
    let target = state::models_dir()?.join(&filename);
    let staging = target.with_extension("bin.part");
    if target.exists() {
        return Err(AppError::Config(format!(
            "model already exists at {}; keeping existing file",
            target.display()
        )));
    }

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
            let client = download_client()?;
            let mut response = client
                .get(&url)
                .send()
                .map_err(|error| AppError::Config(format!("GET {url}: {error}")))?;
            ensure_success(&response, "Whisper model")?;
            let total = response.content_length();
            let mut file = std::fs::File::create(&staging)?;
            let receipt = copy_verified(
                &mut response,
                &mut file,
                total,
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
            )?;
            file.sync_all()?;
            drop(file);

            emit_download(
                &app_clone,
                &download_id,
                &filename,
                ModelDownloadPhaseV1::Verifying,
                receipt.bytes,
                total,
            );
            if receipt.sha256 != expected_sha256 {
                return Err(AppError::Config(format!(
                    "checksum mismatch for {filename}: expected {expected_sha256}, got {}",
                    receipt.sha256
                )));
            }
            if target.exists() {
                return Err(AppError::Config(format!(
                    "model already exists at {}; keeping existing file",
                    target.display()
                )));
            }
            std::fs::rename(&staging, &target)?;
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
        }
        if result.is_err() {
            let _ = std::fs::remove_file(&staging);
        }
        result
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?;
    if let Err(error) = &result {
        crate::events::emit_error(
            &app,
            crate::events::ErrorCodeV1::Download,
            error.to_string(),
            None,
        );
    }
    result
}
