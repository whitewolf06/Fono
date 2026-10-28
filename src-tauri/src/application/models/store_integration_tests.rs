//! Desktop settings integration with an explicitly supplied model cache.

use super::*;

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
        let mut source =
            std::io::BufReader::new(std::fs::File::open(cache.join(&artifact.filename)).unwrap());
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
        let selection = SttModelSelection::from_whisper_path(target.to_string_lossy().into_owned());
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
