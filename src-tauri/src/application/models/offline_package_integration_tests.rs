//! Desktop settings and history integration with explicitly supplied public models.

use super::*;

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
        assert!(matches!(engine.readiness(), SttReadiness::Ready { device } if device == "CPU"));
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
