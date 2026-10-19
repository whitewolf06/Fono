use super::*;
use crate::stt::{SttEngine, SttHealth, SttReadiness, WorkerPaths};
use crate::types::{AccelerationMode, SttModelSelection};
use sha2::{Digest, Sha256};

#[test]
fn pinned_packages_keep_their_head_tokenizer_languages_and_license() {
    let specs = offline_models::specifications().unwrap();
    assert_eq!(specs.len(), 2);
    for spec in &specs {
        assert_eq!(spec.revision.len(), 40);
        assert!(spec.punctuation);
        assert_eq!(spec.encoder, "encoder.int8.onnx");
        for file in &spec.files {
            assert!(file.url.contains(&format!("/resolve/{}/", spec.revision)));
            assert_eq!(file.sha256.len(), 64);
            assert!(file.bytes > 0);
        }
    }
    assert_eq!(specs[0].id.variant, "v3_e2e_rnnt_int8");
    assert_eq!(specs[0].languages, ["ru"]);
    assert_eq!(specs[0].feature_dim, 64);
    assert_eq!(specs[0].license, "MIT");
    assert_eq!(specs[1].languages.len(), 25);
    assert_eq!(specs[1].feature_dim, 80);
    assert_eq!(specs[1].license, "CC-BY-4.0");
}

#[test]
fn package_verification_rejects_corruption_and_cancellation_before_native_loading() {
    let directory = std::env::temp_dir().join(format!(
        "fono-offline-verification-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&directory).unwrap();
    let mut spec = offline_models::specifications().unwrap().remove(0);
    spec.files = vec![offline_models::OfflineArtifact {
        filename: "tokens.txt".into(),
        url: "https://example.invalid/pinned".into(),
        sha256: format!("{:x}", Sha256::digest(b"ok")),
        bytes: 2,
    }];
    std::fs::write(directory.join("tokens.txt"), b"ok").unwrap();
    let cancel = OperationCancellation::default();
    let guards = offline_models::verify_directory(&directory, &spec, &cancel).unwrap();
    #[cfg(windows)]
    assert!(std::fs::write(directory.join("tokens.txt"), b"xx").is_err());
    drop(guards);
    std::fs::write(directory.join("tokens.txt"), b"xx").unwrap();
    assert!(matches!(
        offline_models::verify_directory(&directory, &spec, &cancel),
        Err(AppError::Config(_))
    ));
    cancel.cancel();
    assert!(matches!(
        offline_models::verify_directory(&directory, &spec, &cancel),
        Err(AppError::Cancelled(_))
    ));
    std::fs::remove_file(directory.join("tokens.txt")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn sentencepiece_times_keep_unicode_words_and_absolute_offsets() {
    let tokens = ["▁Привет".into(), ",".into(), "▁мир".into(), "!".into()];
    let words = timed_words(&tokens, Some(&[0.1, 0.2, 0.3, 0.4]), None, 32_000, 16_000).unwrap();
    assert_eq!(words[0].text, "Привет,");
    assert_eq!(words[1].text, "мир!");
    assert_eq!(words[0].start_sample, 33_600);
    assert!(words[1].end_sample <= 48_000);
    assert!(timed_words(&tokens, Some(&[0.4, 0.2, 0.3, 0.4]), None, 0, 16_000).is_err());
    assert!(timed_words(&tokens, None, None, 0, 16_000)
        .unwrap()
        .is_empty());
}

#[test]
fn explicit_gpu_mode_cannot_silently_run_an_offline_cpu_model() {
    let spec = offline_models::specifications().unwrap().remove(0);
    let model = SttModelSelection {
        id: spec.id,
        local_path: env!("CARGO_MANIFEST_DIR").into(),
    };
    let engine = SttEngine::new();
    let error = engine
        .ensure_model_loaded(
            &model,
            AccelerationMode::Cuda,
            &WorkerPaths::default(),
            &OperationCancellation::default(),
        )
        .unwrap_err();
    assert!(matches!(error, AppError::BackendUnavailable(_)));
    assert_eq!(engine.model(), None);
}

#[test]
#[ignore = "requires explicitly downloaded and hash-verified offline model packages"]
fn offline_public_wav_cpu_contract() {
    use crate::application::audio_ingest::{decode_file, AudioIngestPolicy};
    let directory = PathBuf::from(
        std::env::var("FONO_OFFLINE_TEST_MODELS").expect("explicit model cache required"),
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workers =
        WorkerPaths::default().with_speech_vad(root.join("resources/vad/silero_vad.onnx"));
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/fixtures/stt-quality/manifest.json")).unwrap(),
    )
    .unwrap();
    let cancel = OperationCancellation::default();
    for spec in offline_models::specifications().unwrap() {
        let model = SttModelSelection {
            id: spec.id.clone(),
            local_path: directory
                .join(&spec.directory)
                .to_string_lossy()
                .into_owned(),
        };
        let engine = SttEngine::new();
        engine
            .ensure_model_loaded(&model, AccelerationMode::Cpu, &workers, &cancel)
            .unwrap();
        assert_eq!(engine.model(), Some(spec.id.clone()));
        assert_eq!(engine.device(), "CPU");
        assert!(matches!(engine.readiness(), SttReadiness::Ready { device } if device == "CPU"));
        assert!(matches!(engine.health(), SttHealth::Ready { device } if device == "CPU"));
        let capabilities = engine.capabilities().unwrap();
        assert_eq!(
            capabilities.cancellation,
            CancellationSupport::BetweenSegments
        );
        assert!(
            !capabilities.initial_prompt
                && !capabilities.decoding_options
                && !capabilities.language_detection
        );
        for id in if spec.languages.len() == 1 {
            vec!["ru-01"]
        } else {
            vec!["ru-01", "en-01"]
        } {
            let entry = manifest["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["id"] == id)
                .unwrap();
            let path = root
                .join("tests/fixtures/stt-quality")
                .join(entry["path"].as_str().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
                entry["sha256"].as_str().unwrap()
            );
            let audio = decode_file(&path, AudioIngestPolicy::default()).unwrap();
            let result = engine
                .transcribe_window(
                    &audio.pcm_samples,
                    entry["language"].as_str().unwrap(),
                    None,
                    42,
                    &cancel,
                    32_000,
                )
                .unwrap();
            assert!(!result.text.is_empty());
            assert!(!result.words.is_empty());
            assert_eq!(result.backend, BackendKind::Cpu);
            assert!(result.words.iter().all(|word| word.start_sample >= 32_000
                && word.end_sample <= 32_000 + audio.pcm_samples.len() as u64));
            eprintln!("{} {id}: {}", spec.directory, result.text);
            if id == "ru-01" {
                let mut long = Vec::new();
                for _ in 0..4 {
                    long.extend_from_slice(&audio.pcm_samples);
                    long.extend(std::iter::repeat(0).take(16_000));
                }
                let result = engine
                    .transcribe_cancellable(&long, "ru", cancel.clone())
                    .unwrap();
                assert_eq!(result.device.as_deref(), Some("CPU"));
                assert_eq!(result.audio_secs, Some(long.len() as f32 / 16_000.0));
                assert!(!result.text.is_empty());
                eprintln!(
                    "{} long {:.2}s: {}",
                    spec.directory,
                    result.audio_secs.unwrap(),
                    result.text
                );
            }
        }
        let cancelled = OperationCancellation::default();
        cancelled.cancel();
        assert!(matches!(
            engine.transcribe_cancellable(&[0; 160], "ru", cancelled),
            Err(AppError::Cancelled(_))
        ));
        assert!(matches!(engine.health(), SttHealth::Ready { .. }));
        assert!(!engine.gpu_residency().resident);
    }
}
