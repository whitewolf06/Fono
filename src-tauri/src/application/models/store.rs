//! Atomic publication/deletion seams, tested without network or model loads.
use super::download::{copy_verified, DownloadReceipt};
use crate::error::{AppError, AppResult};
use std::{
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn install_verified_model(
    reader: &mut impl Read,
    target: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    cancellation: &AtomicBool,
    progress: impl FnMut(u64),
    verifying: impl FnOnce(),
) -> AppResult<DownloadReceipt> {
    if target.exists() {
        return Err(AppError::Config(
            "Model already installed; existing file retained".into(),
        ));
    }
    let staging = target.with_extension("bin.part");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    let result = (|| {
        let receipt = copy_verified(
            reader,
            &mut file,
            Some(expected_bytes),
            cancellation,
            progress,
        )?;
        verifying();
        if receipt.sha256 != expected_sha256 {
            return Err(AppError::Config(
                "Model checksum mismatch; download discarded".into(),
            ));
        }
        file.sync_all()?;
        drop(file);
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        // Same-directory publication cannot overwrite a racing target file.
        std::fs::hard_link(&staging, target)?;
        Ok(receipt)
    })();
    // Only the staging file created by this invocation is removed.
    if let Err(error) = std::fs::remove_file(&staging) {
        tracing::warn!(%error, "model staging cleanup failed");
    }
    result
}

pub(super) fn remove_model_file(target: &Path, selected: Option<&str>) -> AppResult<()> {
    if let Some(selected) = selected {
        if Path::new(selected) == target
            || std::fs::canonicalize(selected)
                .ok()
                .is_some_and(|path| std::fs::canonicalize(target).ok().as_ref() == Some(&path))
        {
            return Err(AppError::Config(
                "Select another speech model before removing this one".into(),
            ));
        }
    }
    std::fs::remove_file(target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{io::Cursor, sync::atomic::AtomicU64};
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    #[test]
    #[ignore = "requires explicitly downloaded pinned FONO-118 models"]
    fn quantized_artifacts_install_and_load_with_typed_settings() {
        use crate::types::{AccelerationMode, Settings, SttModelSelection, WhisperArtifact};
        let cache = std::path::PathBuf::from(
            std::env::var("FONO_QUANTIZED_TEST_MODELS").expect("explicit model cache required"),
        );
        let staging =
            std::env::temp_dir().join(format!("fono-quantized-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&staging).unwrap();
        for artifact in WhisperArtifact::all().unwrap().into_iter().skip(6) {
            let target = staging.join(&artifact.filename);
            let mut source = std::io::BufReader::new(
                std::fs::File::open(cache.join(&artifact.filename)).unwrap(),
            );
            let receipt = install_verified_model(
                &mut source,
                &target,
                artifact.bytes,
                &artifact.sha256,
                &AtomicBool::new(false),
                |_| {},
                || {},
            )
            .unwrap();
            assert_eq!(receipt.bytes, artifact.bytes);
            let selection =
                SttModelSelection::from_whisper_path(target.to_string_lossy().into_owned());
            assert_eq!(selection.id, artifact.id());
            let mut settings = Settings {
                stt_model: Some(selection.clone()),
                ..Settings::default()
            };
            settings.reconcile_stt_model(None).unwrap();
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.selected_stt_model(), Some(selection.clone()));
            let engine = crate::stt::SttEngine::new();
            engine
                .ensure_model_loaded(
                    &selection,
                    AccelerationMode::Cpu,
                    &crate::stt::WorkerPaths::default(),
                    &fono_core::OperationCancellation::default(),
                )
                .unwrap();
            assert_eq!(engine.model(), Some(artifact.id()));
            assert_eq!(engine.device(), "CPU");
            assert!(remove_model_file(&target, Some(&selection.local_path)).is_err());
            drop(engine);
            remove_model_file(&target, None).unwrap();
        }
        std::fs::remove_dir(staging).unwrap();
    }
    fn target() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fono-model-store-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        dir.join("model.bin")
    }
    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("bin.part"));
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn verified_file_is_published_and_only_an_unselected_model_can_be_deleted() {
        let path = target();
        let bytes = b"test model";
        install_verified_model(
            &mut Cursor::new(bytes),
            &path,
            bytes.len() as u64,
            &digest(bytes),
            &AtomicBool::new(false),
            |_| {},
            || {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!path.with_extension("bin.part").exists());
        assert!(remove_model_file(&path, path.to_str()).is_err());
        assert!(path.exists());
        remove_model_file(&path, None).unwrap();
        assert!(!path.exists());
        cleanup(&path);
    }
    #[test]
    fn wrong_hash_size_or_cancellation_never_publishes_a_model() {
        for (size, hash, cancelled) in [
            (5, digest(b"wrong"), false),
            (10, digest(b"model"), false),
            (5, digest(b"model"), true),
        ] {
            let path = target();
            assert!(install_verified_model(
                &mut Cursor::new(b"model"),
                &path,
                size,
                &hash,
                &AtomicBool::new(cancelled),
                |_| {},
                || {}
            )
            .is_err());
            assert!(!path.exists());
            assert!(!path.with_extension("bin.part").exists());
            cleanup(&path);
        }
    }
    #[test]
    fn racing_existing_file_is_never_overwritten() {
        let path = target();
        let published = path.clone();
        let bytes = b"model";
        assert!(install_verified_model(
            &mut Cursor::new(bytes),
            &path,
            bytes.len() as u64,
            &digest(bytes),
            &AtomicBool::new(false),
            |_| {},
            || {
                std::fs::write(&published, b"existing").unwrap();
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        assert!(!path.with_extension("bin.part").exists());
        cleanup(&path);
    }
}
