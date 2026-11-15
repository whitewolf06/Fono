//! Each multi-file package becomes visible only after every artifact is verified.
use super::{
    download::{download_client, emit_download, ensure_success, DownloadPhase, DownloadProgress},
    store,
};
use crate::{
    error::{SttError as AppError, SttResult as AppResult},
    offline_models::{OfflineArtifact, OfflineModelSpec},
};
use std::{
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const NOTICE: &str = include_str!("../../../../licenses/offline-stt-NOTICE.md");

pub fn is_installed(path: &Path, spec: &OfflineModelSpec) -> bool {
    // Listing is cheap. Activation always rechecks SHA256 under native file guards.
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        && spec.files.iter().all(|f| {
            std::fs::symlink_metadata(path.join(&f.filename))
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() == f.bytes)
        })
        && path.join("FONO-MODEL-NOTICE.md").is_file()
}

pub fn install(
    target: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    mut fetch: impl FnMut(&OfflineArtifact) -> AppResult<Box<dyn Read>>,
    mut progress: impl FnMut(DownloadPhase, u64),
) -> AppResult<()> {
    if target.exists() {
        return Err(AppError::Config(
            "Model package already exists; existing files retained".into(),
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| AppError::Config("Model package has no parent".into()))?;
    let staging = parent.join(format!(".fono-model-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging)?;
    let result = (|| {
        let mut completed = 0;
        for artifact in &spec.files {
            if cancellation.load(Ordering::Acquire) {
                return Err(AppError::Cancelled("model download cancelled".into()));
            }
            if artifact.filename.contains(['/', '\\', ':'])
                || matches!(artifact.filename.as_str(), "" | "." | "..")
            {
                return Err(AppError::Config("Invalid model artifact filename".into()));
            }
            let mut reader = fetch(artifact)?;
            store::install_verified_model(
                &mut reader,
                &staging.join(&artifact.filename),
                artifact.bytes,
                &artifact.sha256,
                cancellation,
                |bytes| progress(DownloadPhase::Downloading, completed + bytes),
                || {},
            )?;
            completed += artifact.bytes;
        }
        progress(DownloadPhase::Verifying, completed);
        let mut notice = std::fs::File::create(staging.join("FONO-MODEL-NOTICE.md"))?;
        use std::io::Write;
        notice.write_all(NOTICE.as_bytes())?;
        notice.sync_all()?;
        drop(notice);
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        if target.exists() {
            return Err(AppError::Config(
                "Model package appeared during download; existing files retained".into(),
            ));
        }
        // Windows rename never replaces an existing directory. The model ID
        // registration serializes all Fono download/removal operations.
        std::fs::rename(&staging, target)?;
        Ok(())
    })();
    // Only files inside the unique directory created by this invocation.
    if staging.exists() {
        for artifact in &spec.files {
            if !artifact.filename.contains(['/', '\\', ':'])
                && !matches!(artifact.filename.as_str(), "" | "." | "..")
            {
                let _ = std::fs::remove_file(staging.join(&artifact.filename));
            }
        }
        let _ = std::fs::remove_file(staging.join("FONO-MODEL-NOTICE.md"));
        let _ = std::fs::remove_dir(&staging);
    }
    result
}

/// Download and atomically publish a pinned offline model package.
pub fn download(
    models_dir: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
) -> AppResult<()> {
    let download_id = spec.id.public_id();
    let total = spec.files.iter().map(|file| file.bytes).sum();
    let emit = |phase, bytes| {
        emit_download(
            progress,
            &download_id,
            &spec.directory,
            phase,
            bytes,
            Some(total),
        )
    };
    emit(DownloadPhase::Started, 0);
    let result = (|| {
        let target = models_dir.join(&spec.directory);
        let client = download_client()?;
        install(
            &target,
            spec,
            cancellation,
            |artifact| {
                let response = client
                    .get(&artifact.url)
                    .send()
                    .map_err(|error| AppError::Config(format!("Model download: {error}")))?;
                ensure_success(&response, "offline model")?;
                Ok(Box::new(response))
            },
            emit,
        )
    })();
    match &result {
        Ok(()) => emit(DownloadPhase::Completed, total),
        Err(AppError::Cancelled(_)) => emit(DownloadPhase::Cancelled, 0),
        Err(_) => emit(DownloadPhase::Failed, 0),
    }
    result
}

pub fn remove(target: &Path, spec: &OfflineModelSpec, selected: Option<&str>) -> AppResult<()> {
    if selected.is_some_and(|selected| {
        Path::new(selected) == target
            || std::fs::canonicalize(selected)
                .ok()
                .is_some_and(|p| std::fs::canonicalize(target).ok().as_ref() == Some(&p))
    }) {
        return Err(AppError::Config(
            "Сначала выберите другую модель распознавания".into(),
        ));
    }
    let metadata = std::fs::symlink_metadata(target)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::Config("Invalid model package directory".into()));
    }
    // No recursive removal, no traversal of unknown files or junctions.
    for artifact in &spec.files {
        std::fs::remove_file(target.join(&artifact.filename))?;
    }
    let _ = std::fs::remove_file(target.join("FONO-MODEL-NOTICE.md"));
    std::fs::remove_dir(target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn fixture() -> (std::path::PathBuf, OfflineModelSpec) {
        let root =
            std::env::temp_dir().join(format!("fono-offline-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut spec = crate::offline_models::specifications().unwrap().remove(0);
        spec.files.truncate(2);
        for file in &mut spec.files {
            file.bytes = 3;
            file.sha256 = format!("{:x}", Sha256::digest(b"abc"));
        }
        (root.join("package"), spec)
    }
    #[test]
    fn package_publication_is_complete_verified_and_does_not_overwrite() {
        let (target, spec) = fixture();
        let cancel = AtomicBool::new(false);
        install(
            &target,
            &spec,
            &cancel,
            |_| Ok(Box::new(std::io::Cursor::new(b"abc"))),
            |_, _| {},
        )
        .unwrap();
        assert!(is_installed(&target, &spec));
        assert!(install(
            &target,
            &spec,
            &cancel,
            |_| panic!("must not fetch"),
            |_, _| {}
        )
        .is_err());
        assert!(remove(&target, &spec, target.to_str()).is_err());
        remove(&target, &spec, None).unwrap();
        std::fs::remove_dir(target.parent().unwrap()).unwrap();
    }
    #[test]
    fn corrupt_or_cancelled_package_never_becomes_visible_and_cleans_only_its_staging() {
        for cancel in [false, true] {
            let (target, spec) = fixture();
            assert!(install(
                &target,
                &spec,
                &AtomicBool::new(cancel),
                |_| Ok(Box::new(std::io::Cursor::new(b"bad"))),
                |_, _| {}
            )
            .is_err());
            assert!(!target.exists());
            assert_eq!(
                std::fs::read_dir(target.parent().unwrap()).unwrap().count(),
                0
            );
            std::fs::remove_dir(target.parent().unwrap()).unwrap();
        }
    }
}
