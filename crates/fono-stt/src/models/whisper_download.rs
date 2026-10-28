//! Blocking Whisper download; the caller owns scheduling and admission.

use super::{
    download::{download_client, emit_download, ensure_success, DownloadPhase, DownloadProgress},
    store::install_verified_model,
};
use crate::error::{SttError as AppError, SttResult as AppResult};
use fono_types::WhisperArtifact;
use std::{path::Path, sync::atomic::AtomicBool};

pub fn download_whisper_artifact(
    models_dir: &Path,
    artifact: WhisperArtifact,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
) -> AppResult<()> {
    let filename = artifact.filename;
    let download_id = format!("whisper:{}", artifact.variant);
    let expected_bytes = artifact.bytes;
    emit_download(
        progress,
        &download_id,
        &filename,
        DownloadPhase::Started,
        0,
        None,
    );
    let result = (|| {
        let target = models_dir.join(&filename);
        if target.exists() {
            return Err(AppError::Config(
                "Model already exists; keeping existing file".into(),
            ));
        }
        let client = download_client()?;
        let mut response = client
            .get(&artifact.url)
            .send()
            .map_err(|error| AppError::Config(format!("GET {}: {error}", artifact.url)))?;
        ensure_success(&response, "Whisper model")?;
        let total = response.content_length();
        let receipt = install_verified_model(
            &mut response,
            &target,
            expected_bytes,
            &artifact.sha256,
            cancellation,
            |bytes| {
                emit_download(
                    progress,
                    &download_id,
                    &filename,
                    DownloadPhase::Downloading,
                    bytes,
                    total,
                )
            },
            || {
                emit_download(
                    progress,
                    &download_id,
                    &filename,
                    DownloadPhase::Verifying,
                    expected_bytes,
                    Some(expected_bytes),
                )
            },
        )?;
        emit_download(
            progress,
            &download_id,
            &filename,
            DownloadPhase::Completed,
            receipt.bytes,
            total,
        );
        Ok(())
    })();
    match &result {
        Err(AppError::Cancelled(_)) => emit_download(
            progress,
            &download_id,
            &filename,
            DownloadPhase::Cancelled,
            0,
            None,
        ),
        Err(_) => emit_download(
            progress,
            &download_id,
            &filename,
            DownloadPhase::Failed,
            0,
            None,
        ),
        Ok(()) => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[test]
    fn existing_file_fails_without_network_and_emits_one_terminal_phase() {
        let directory =
            std::env::temp_dir().join(format!("fono-whisper-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let artifact = WhisperArtifact::from_size(fono_types::WhisperModelSize::Tiny);
        let target = directory.join(&artifact.filename);
        std::fs::write(&target, b"existing file").unwrap();
        let events = Mutex::new(Vec::new());
        let progress = |event| events.lock().push(event);
        assert!(matches!(
            download_whisper_artifact(&directory, artifact, &AtomicBool::new(false), &progress),
            Err(AppError::Config(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"existing file");
        let events = events.lock();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].phase, DownloadPhase::Started);
        assert_eq!(events[1].phase, DownloadPhase::Failed);
        assert!(events
            .iter()
            .all(|event| event.download_id == "whisper:tiny"));
        drop(events);
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
