//! Application use cases for downloading and selecting speech and wake models.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::events::{ModelDownloadEventV1, ModelDownloadPhaseV1};
use crate::state::{self, AppState};
use crate::types::{WhisperModelInfo, WhisperModelSize};

const KWS_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";
const KWS_MODEL_DIR: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01";
const KWS_ARCHIVE: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2";
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const PROGRESS_STEP_BYTES: u64 = 1024 * 1024;

static ACTIVE_DOWNLOADS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct DownloadRegistration {
    id: String,
    cancellation: Arc<AtomicBool>,
}

impl DownloadRegistration {
    fn begin(id: String) -> AppResult<Self> {
        let mut active = ACTIVE_DOWNLOADS.lock();
        if active.contains_key(&id) {
            return Err(AppError::Busy(format!(
                "model download {id} is already active"
            )));
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        active.insert(id.clone(), Arc::clone(&cancellation));
        Ok(Self { id, cancellation })
    }
}

impl Drop for DownloadRegistration {
    fn drop(&mut self) {
        ACTIVE_DOWNLOADS.lock().remove(&self.id);
    }
}

struct DownloadReceipt {
    bytes: u64,
    sha256: String,
}

pub fn cancel_download(download_id: &str) -> bool {
    let Some(cancellation) = ACTIVE_DOWNLOADS.lock().get(download_id).cloned() else {
        return false;
    };
    cancellation.store(true, Ordering::Release);
    true
}

/// Starts a best-effort background preload of the model selected in persisted
/// settings.  It deliberately does not make application startup fail: the
/// typed STT readiness IPC remains the source of truth for a missing model or
/// unavailable worker, and dictation still uses the same load gate.
pub fn preload_configured_stt(app: AppHandle) {
    let settings = app.state::<AppState>().settings();
    let Some((model_path, acceleration)) = configured_stt_preload(&settings) else {
        return;
    };
    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    let worker_paths = crate::stt::worker_paths_for_app(&app);

    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
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

pub fn is_kws_model_downloaded() -> AppResult<bool> {
    let dir = state::app_data_dir()?
        .join("kws-models")
        .join(KWS_MODEL_DIR);
    Ok(kws_model_is_complete(&dir))
}

pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    let download_id = "kws".to_string();
    let registration = DownloadRegistration::begin(download_id.clone())?;
    let base_dir = state::app_data_dir()?.join("kws-models");
    std::fs::create_dir_all(&base_dir)?;
    let target = base_dir.join(KWS_MODEL_DIR);
    if target.exists() {
        return if kws_model_is_complete(&target) {
            Ok(())
        } else {
            Err(AppError::Config(format!(
                "incomplete KWS model exists at {}; remove it before retrying",
                target.display()
            )))
        };
    }
    let staging_dir = base_dir.join(format!(".{KWS_MODEL_DIR}.staging"));
    let archive_path = staging_dir.join(KWS_ARCHIVE);
    let filename = KWS_MODEL_DIR.to_string();
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
        if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir)?;
        }
        std::fs::create_dir_all(&staging_dir)?;
        let result = (|| -> AppResult<()> {
            let client = download_client()?;
            let mut response = client
                .get(KWS_MODEL_URL)
                .send()
                .map_err(|error| AppError::Config(format!("GET {KWS_MODEL_URL}: {error}")))?;
            ensure_success(&response, "KWS model")?;
            let total = response.content_length();
            let mut file = std::fs::File::create(&archive_path)?;
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
                ModelDownloadPhaseV1::Extracting,
                receipt.bytes,
                total,
            );
            let file = std::fs::File::open(&archive_path)?;
            let decompress = bzip2::read::BzDecoder::new(file);
            tar::Archive::new(decompress)
                .unpack(&staging_dir)
                .map_err(|error| AppError::Config(format!("unpack: {error}")))?;
            if registration.cancellation.load(Ordering::Acquire) {
                return Err(AppError::Cancelled("KWS model download cancelled".into()));
            }
            let staged_model = staging_dir.join(KWS_MODEL_DIR);
            if !kws_model_is_complete(&staged_model) {
                return Err(AppError::Config(
                    "KWS archive is missing required model files".into(),
                ));
            }
            std::fs::rename(&staged_model, &target)?;
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
        let _ = std::fs::remove_dir_all(&staging_dir);
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

pub fn set_whisper_model(state: &AppState, path: String) -> AppResult<()> {
    let mut settings = state.settings();
    settings.whisper_model_path = Some(path);
    state::save_settings(&settings)?;
    state.set_settings(settings);
    Ok(())
}

fn download_client() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| AppError::Config(error.to_string()))
}

fn ensure_success(response: &reqwest::blocking::Response, label: &str) -> AppResult<()> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Config(format!(
            "HTTP {} while downloading {label}",
            response.status()
        )))
    }
}

fn copy_verified<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    expected_bytes: Option<u64>,
    cancellation: &AtomicBool,
    mut progress: F,
) -> AppResult<DownloadReceipt>
where
    R: Read,
    W: Write,
    F: FnMut(u64),
{
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut bytes = 0_u64;
    let mut next_progress = PROGRESS_STEP_BYTES;
    let mut hasher = Sha256::new();
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
        if bytes >= next_progress {
            progress(bytes);
            next_progress = bytes.saturating_add(PROGRESS_STEP_BYTES);
        }
    }
    if bytes == 0 {
        return Err(AppError::Config(
            "model download returned an empty file".into(),
        ));
    }
    if let Some(expected) = expected_bytes {
        if expected != bytes {
            return Err(AppError::Config(format!(
                "download size mismatch: expected {expected} bytes, got {bytes}"
            )));
        }
    }
    progress(bytes);
    Ok(DownloadReceipt {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn emit_download(
    app: &AppHandle,
    download_id: &str,
    model: &str,
    phase: ModelDownloadPhaseV1,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    crate::events::emit_model_download(
        app,
        ModelDownloadEventV1 {
            download_id: download_id.to_string(),
            model: model.to_string(),
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
}

fn kws_model_is_complete(dir: &Path) -> bool {
    let encoder = dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let decoder = dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let joiner = dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let tokens = dir.join("tokens.txt");
    encoder.is_file() && decoder.is_file() && joiner.is_file() && tokens.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn verified_copy_reports_size_and_checksum() {
        let input = b"verified model bytes";
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let receipt = copy_verified(
            &mut reader,
            &mut output,
            Some(input.len() as u64),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(receipt.bytes, input.len() as u64);
        assert_eq!(output, input);
        assert_eq!(receipt.sha256.len(), 64);
    }

    #[test]
    fn verified_copy_rejects_wrong_size() {
        let mut reader = Cursor::new(b"short");
        let mut output = Vec::new();
        assert!(copy_verified(
            &mut reader,
            &mut output,
            Some(10),
            &AtomicBool::new(false),
            |_| {},
        )
        .is_err());
    }

    #[test]
    fn verified_copy_honors_cancellation_before_writing() {
        let mut reader = Cursor::new(b"model");
        let mut output = Vec::new();
        assert!(matches!(
            copy_verified(
                &mut reader,
                &mut output,
                None,
                &AtomicBool::new(true),
                |_| {},
            ),
            Err(AppError::Cancelled(_))
        ));
        assert!(output.is_empty());
    }

    #[test]
    fn preload_request_uses_the_persisted_model_and_acceleration() {
        let settings = crate::types::Settings {
            whisper_model_path: Some("C:/models/ggml-small.bin".into()),
            acceleration: crate::types::AccelerationMode::Vulkan,
            ..Default::default()
        };

        assert_eq!(
            configured_stt_preload(&settings),
            Some((
                std::path::PathBuf::from("C:/models/ggml-small.bin"),
                crate::types::AccelerationMode::Vulkan,
            ))
        );
        assert_eq!(configured_stt_preload(&Default::default()), None);
    }
}
