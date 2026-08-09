//! IPC commands for downloading and selecting speech and wake-word models.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::types::{WhisperModelInfo, WhisperModelSize};

const KWS_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";
const KWS_MODEL_DIR: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01";
const KWS_ARCHIVE: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";

#[tauri::command]
pub fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    let dir = state::models_dir()?;
    let mut out = Vec::new();
    for size in [
        WhisperModelSize::Tiny,
        WhisperModelSize::Base,
        WhisperModelSize::Small,
        WhisperModelSize::Medium,
        WhisperModelSize::Large,
    ] {
        let filename = size.filename();
        let path = dir.join(filename);
        let (local_path, bytes) = if path.exists() {
            let meta = std::fs::metadata(&path).ok();
            (
                Some(path.to_string_lossy().to_string()),
                meta.map(|m| m.len()),
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

#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, size: String) -> AppResult<()> {
    let model_size: WhisperModelSize =
        serde_json::from_value(serde_json::Value::String(size.clone()))
            .map_err(|_| AppError::Config(format!("неизвестный размер модели: {size}")))?;
    let url = model_size.url().to_string();
    let target: PathBuf = state::models_dir()?.join(model_size.filename());
    let staging = target.with_extension("bin.part");

    tracing::info!("downloading {} -> {}", url, target.display());

    let app_clone = app.clone();
    let filename = model_size.filename().to_string();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| AppError::Config(e.to_string()))?;
        let mut resp = client
            .get(&url)
            .send()
            .map_err(|e| AppError::Config(format!("GET {url}: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Config(format!(
                "HTTP {} при скачивании модели",
                resp.status()
            )));
        }
        let transfer = (|| -> AppResult<()> {
            let mut file = std::fs::File::create(&staging)?;
            let bytes = resp
                .copy_to(&mut file)
                .map_err(|e| AppError::Config(format!("copy_to: {e}")))?;
            if bytes == 0 {
                return Err(AppError::Config(
                    "model download returned an empty file".into(),
                ));
            }
            file.sync_all()?;
            drop(file);
            if target.exists() {
                return Err(AppError::Config(format!(
                    "model already exists at {}; keeping existing file",
                    target.display()
                )));
            }
            std::fs::rename(&staging, &target)?;
            Ok(())
        })();
        if transfer.is_err() {
            let _ = std::fs::remove_file(&staging);
        }
        transfer?;
        tracing::info!("model activated: {}", target.display());
        let _ = app_clone.emit("model-downloaded", filename);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(())
}

#[tauri::command]
pub fn is_kws_model_downloaded() -> AppResult<bool> {
    let dir = state::app_data_dir()?
        .join("kws-models")
        .join(KWS_MODEL_DIR);
    Ok(kws_model_is_complete(&dir))
}

fn kws_model_is_complete(dir: &Path) -> bool {
    let encoder = dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let decoder = dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let joiner = dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let tokens = dir.join("tokens.txt");
    encoder.is_file() && decoder.is_file() && joiner.is_file() && tokens.is_file()
}

#[tauri::command]
pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    let base_dir = state::app_data_dir()?.join("kws-models");
    std::fs::create_dir_all(&base_dir)?;
    let staging_dir = base_dir.join(format!(".{KWS_MODEL_DIR}.staging"));
    let archive_path = staging_dir.join(KWS_ARCHIVE);
    let url = KWS_MODEL_URL.to_string();

    tracing::info!(
        "downloading KWS model {} -> {}",
        url,
        archive_path.display()
    );
    let app_clone = app.clone();

    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir)?;
        }
        std::fs::create_dir_all(&staging_dir)?;
        let result = (|| -> AppResult<()> {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .map_err(|e| AppError::Config(e.to_string()))?;
            let mut resp = client
                .get(&url)
                .send()
                .map_err(|e| AppError::Config(format!("GET {url}: {e}")))?;
            if !resp.status().is_success() {
                return Err(AppError::Config(format!(
                    "HTTP {} при скачивании KWS-модели",
                    resp.status()
                )));
            }
            let mut file = std::fs::File::create(&archive_path)?;
            resp.copy_to(&mut file)
                .map_err(|e| AppError::Config(format!("copy_to: {e}")))?;
            drop(file);

            tracing::info!("extracting KWS model to {}", staging_dir.display());
            let file = std::fs::File::open(&archive_path)?;
            let decompress = bzip2::read::BzDecoder::new(file);
            let mut archive = tar::Archive::new(decompress);
            archive
                .unpack(&staging_dir)
                .map_err(|e| AppError::Config(format!("unpack: {e}")))?;

            let staged_model = staging_dir.join(KWS_MODEL_DIR);
            if !kws_model_is_complete(&staged_model) {
                return Err(AppError::Config(
                    "KWS archive is missing required model files".into(),
                ));
            }
            let target = base_dir.join(KWS_MODEL_DIR);
            if target.exists() {
                return Err(AppError::Config(
                    "KWS model already exists; keeping current model".into(),
                ));
            }
            std::fs::rename(&staged_model, &target)?;
            tracing::info!("KWS model ready");
            let _ = app_clone.emit("kws-model-downloaded", true);
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&staging_dir);
        result
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(())
}

#[tauri::command]
pub fn set_whisper_model(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let mut settings = state.settings();
    settings.whisper_model_path = Some(path);
    state::save_settings(&settings)?;
    state.set_settings(settings);
    Ok(())
}
