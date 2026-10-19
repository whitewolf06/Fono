//! Each multi-file package becomes visible only after every artifact is verified.
use super::{
    download::{download_client, emit_download, ensure_success, DownloadRegistration},
    store,
};
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
    stt::offline_models::{OfflineArtifact, OfflineModelSpec},
    types::ModelId,
};
use std::{
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::AppHandle;

const NOTICE: &str = include_str!("../../../../docs/evidence/licenses/offline-stt-NOTICE.md");

pub(super) fn is_installed(path: &Path, spec: &OfflineModelSpec) -> bool {
    // Listing is cheap. Activation always rechecks SHA256 under native file guards.
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        && spec.files.iter().all(|f| {
            std::fs::symlink_metadata(path.join(&f.filename))
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() == f.bytes)
        })
        && path.join("FONO-MODEL-NOTICE.md").is_file()
}

fn install(
    target: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    mut fetch: impl FnMut(&OfflineArtifact) -> AppResult<Box<dyn Read>>,
    mut progress: impl FnMut(ModelDownloadPhaseV1, u64),
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
                |bytes| progress(ModelDownloadPhaseV1::Downloading, completed + bytes),
                || {},
            )?;
            completed += artifact.bytes;
        }
        progress(ModelDownloadPhaseV1::Verifying, completed);
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

pub(super) async fn download(app: AppHandle, id: ModelId) -> AppResult<()> {
    let spec = crate::stt::offline_models::specification(&id)?;
    let download_id = id.public_id();
    let registration = DownloadRegistration::begin(download_id.clone())?;
    let total = spec.files.iter().map(|f| f.bytes).sum();
    // Disk access, network transfer, verification and admission share one owner.
    tauri::async_runtime::spawn_blocking(move || {
        let app_copy = app.clone();
        let emit = |phase, bytes| {
            emit_download(
                &app_copy,
                &download_id,
                &spec.directory,
                phase,
                bytes,
                Some(total),
            )
        };
        emit(ModelDownloadPhaseV1::Started, 0);
        let result = (|| {
            let target = crate::state::models_dir()?.join(&spec.directory);
            let client = download_client()?;
            install(
                &target,
                &spec,
                &registration.cancellation,
                |artifact| {
                    let response = client
                        .get(&artifact.url)
                        .send()
                        .map_err(|e| AppError::Config(format!("Model download: {e}")))?;
                    ensure_success(&response, "offline model")?;
                    Ok(Box::new(response))
                },
                emit,
            )
        })();
        match &result {
            Ok(()) => emit(ModelDownloadPhaseV1::Completed, total),
            Err(AppError::Cancelled(_)) => emit(ModelDownloadPhaseV1::Cancelled, 0),
            Err(error) => {
                emit(ModelDownloadPhaseV1::Failed, 0);
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
    .map_err(|_| AppError::Internal("model package download task failed".into()))?
}

pub(super) fn remove(
    target: &Path,
    spec: &OfflineModelSpec,
    selected: Option<&str>,
) -> AppResult<()> {
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
        let mut spec = crate::stt::offline_models::specifications()
            .unwrap()
            .remove(0);
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

    #[test]
    #[ignore = "requires the explicitly downloaded pinned public model cache"]
    fn installed_packages_roundtrip_settings_and_recognize_public_wav() {
        use crate::{
            application::audio_ingest::{decode_file, AudioIngestPolicy},
            stt::{SttEngine, SttReadiness, WorkerPaths},
            types::{DictationBackend, DictationHistoryMetadata, Settings, SttModelSelection},
        };
        let cache = std::path::PathBuf::from(
            std::env::var("FONO_OFFLINE_TEST_MODELS").expect("explicit cache required"),
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workers =
            WorkerPaths::default().with_speech_vad(root.join("resources/vad/silero_vad.onnx"));
        let audio = decode_file(
            &root.join("tests/fixtures/stt-quality/ru-01.wav"),
            AudioIngestPolicy::default(),
        )
        .unwrap();
        let temporary = root
            .parent()
            .unwrap()
            .join("build/checks")
            .join(format!("fono117-package-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temporary).unwrap();
        for spec in crate::stt::offline_models::specifications().unwrap() {
            let target = temporary.join(&spec.directory);
            install(
                &target,
                &spec,
                &AtomicBool::new(false),
                |artifact| {
                    Ok(Box::new(std::fs::File::open(
                        cache.join(&spec.directory).join(&artifact.filename),
                    )?))
                },
                |_, _| {},
            )
            .unwrap();
            assert!(is_installed(&target, &spec));
            let mut settings = Settings {
                stt_model: Some(SttModelSelection {
                    id: spec.id.clone(),
                    local_path: target.to_string_lossy().into_owned(),
                }),
                language: "auto".into(),
                ..Settings::default()
            };
            settings.reconcile_stt_model(None).unwrap();
            let settings: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            settings.validate_stt_preferences().unwrap();
            assert!(settings.whisper_model_path.is_none());
            let engine = SttEngine::new();
            engine
                .ensure_model_loaded(
                    &settings.selected_stt_model().unwrap(),
                    settings.acceleration,
                    &workers,
                    &fono_core::OperationCancellation::default(),
                )
                .unwrap();
            assert_eq!(engine.model(), Some(spec.id.clone()));
            assert!(
                matches!(engine.readiness(), SttReadiness::Ready { device } if device == "CPU")
            );
            let transcript = engine
                .transcribe_cancellable_with_options(
                    &audio.pcm_samples,
                    &settings.language,
                    fono_core::OperationCancellation::default(),
                    settings.decoding_options(),
                )
                .unwrap();
            assert!(!transcript.text.trim().is_empty());
            let metadata =
                DictationHistoryMetadata::from_result(&settings, &transcript, Default::default());
            assert_eq!(metadata.backend, Some(DictationBackend::Cpu));
            assert_eq!(metadata.recognizer, Some(spec.id.engine));
            assert_eq!(metadata.model, Some(spec.id.public_id()));
            eprintln!("{}: CPU / {}", spec.id.public_id(), transcript.text);
            drop(engine);
            remove(&target, &spec, None).unwrap();
        }
        std::fs::remove_dir(temporary).unwrap();
    }
}
