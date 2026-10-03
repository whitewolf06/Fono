use super::super::download::emit_download;
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

fn cancelled(cancellation: &AtomicBool) -> AppResult<()> {
    if cancellation.load(Ordering::Acquire) {
        Err(AppError::Cancelled(
            "WakeWord model download cancelled".into(),
        ))
    } else {
        Ok(())
    }
}
pub(super) async fn fetch_verified_archive(
    app: &AppHandle,
    spec: &fono_wake::WakeModelSpec,
    path: &Path,
    cancellation: &AtomicBool,
) -> AppResult<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| AppError::Config(e.to_string()))?;
    let request = client.get(spec.archive_url).send();
    tokio::pin!(request);
    let mut response = loop {
        tokio::select! {result=&mut request=>break result.map_err(|e|AppError::Config(format!("WakeWord download: {e}")))?,_=tokio::time::sleep(Duration::from_millis(100))=>cancelled(cancellation)?}
    };
    if !response.status().is_success() {
        return Err(AppError::Config(format!(
            "WakeWord download HTTP {}",
            response.status()
        )));
    }
    let mut file = tokio::fs::File::create(path).await?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut next_progress = 0u64;
    loop {
        let chunk = response.chunk();
        tokio::pin!(chunk);
        let chunk = loop {
            tokio::select! {result=&mut chunk=>break result.map_err(|e|AppError::Config(format!("WakeWord download: {e}")))?,_=tokio::time::sleep(Duration::from_millis(100))=>cancelled(cancellation)?}
        };
        let Some(chunk) = chunk else {
            break;
        };
        cancelled(cancellation)?;
        bytes += chunk.len() as u64;
        if bytes > spec.archive_bytes {
            return Err(AppError::Config(
                "WakeWord archive exceeds the pinned size".into(),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        if bytes >= next_progress {
            emit_download(
                app,
                "kws",
                spec.directory,
                ModelDownloadPhaseV1::Downloading,
                bytes,
                Some(spec.archive_bytes),
            );
            next_progress = bytes + 1024 * 1024;
        }
    }
    file.sync_all().await?;
    drop(file);
    cancelled(cancellation)?;
    emit_download(
        app,
        "kws",
        spec.directory,
        ModelDownloadPhaseV1::Verifying,
        bytes,
        Some(spec.archive_bytes),
    );
    if bytes != spec.archive_bytes {
        return Err(AppError::Config(
            "WakeWord archive size does not match its pinned release".into(),
        ));
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != spec.archive_sha256 {
        return Err(AppError::Config(format!(
            "WakeWord checksum mismatch: expected {}, got {actual}",
            spec.archive_sha256
        )));
    }
    Ok(())
}
pub(super) fn extract(
    archive_path: &Path,
    staging: &Path,
    spec: &fono_wake::WakeModelSpec,
    cancellation: &AtomicBool,
) -> AppResult<()> {
    let archive = std::fs::File::open(archive_path)?;
    let decoder = bzip2::read::BzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    let mut buffer = [0u8; 65536];
    for entry in archive.entries()? {
        cancelled(cancellation)?;
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if !safe_path(&path, spec.directory) {
            return Err(AppError::Config(
                "WakeWord archive contains an unsafe path".into(),
            ));
        }
        let kind = entry.header().entry_type();
        let target = staging.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if !kind.is_file() {
            return Err(AppError::Config(
                "WakeWord archive contains a link or special file".into(),
            ));
        }
        if entry.size() > 512 * 1024 * 1024 {
            return Err(AppError::Config(
                "WakeWord archive file is unexpectedly large".into(),
            ));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&target)?;
        loop {
            cancelled(cancellation)?;
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
        }
        output.sync_all()?;
    }
    Ok(())
}
fn safe_path(path: &Path, directory: &str) -> bool {
    let mut components = path.components();
    if !matches!(components.next(),Some(Component::Normal(value)) if value==directory) {
        return false;
    }
    components.all(|c| matches!(c, Component::Normal(_)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_rejects_absolute_parent_and_cross_model_paths() {
        assert!(safe_path(Path::new("model/model.onnx"), "model"));
        assert!(!safe_path(Path::new("model/../outside"), "model"));
        assert!(!safe_path(Path::new("other/file"), "model"));
        assert!(!safe_path(Path::new("C:/escape"), "model"));
    }
    #[test]
    fn cancellation_is_an_explicit_terminal_error() {
        assert!(matches!(
            cancelled(&AtomicBool::new(true)),
            Err(AppError::Cancelled(_))
        ));
    }
}
