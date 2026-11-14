use super::*;
use parking_lot::Mutex;

#[test]
fn auto_uses_vulkan_and_never_selects_the_optional_cuda_package() {
    let paths = WorkerPaths::for_test_workers(
        Some(PathBuf::from("cuda.exe")),
        Some(PathBuf::from("vulkan.exe")),
    );
    let candidates = paths.candidates(AccelerationMode::Auto);
    assert!(matches!(
        candidates.first(),
        Some(EngineCandidate::Worker {
            backend: BackendKind::Vulkan,
            ..
        })
    ));
    assert!(matches!(
        candidates.last(),
        Some(EngineCandidate::Embedded { use_gpu: false })
    ));
    assert!(!candidates.iter().any(|candidate| matches!(
        candidate,
        EngineCandidate::Worker {
            backend: BackendKind::Cuda,
            ..
        }
    )));
}

#[test]
fn explicit_vulkan_never_adds_cpu_fallback() {
    let paths = WorkerPaths::default();
    assert!(paths
        .candidates(AccelerationMode::Vulkan)
        .iter()
        .all(|candidate| { !matches!(candidate, EngineCandidate::Embedded { use_gpu: false }) }));
}

#[test]
fn installing_only_cuda_keeps_auto_on_cpu_until_cuda_is_selected_explicitly() {
    let paths = WorkerPaths::for_test_workers(Some(PathBuf::from("cuda.exe")), None);
    assert!(paths
        .candidates(AccelerationMode::Auto)
        .iter()
        .all(|candidate| !matches!(
            candidate,
            EngineCandidate::Worker {
                backend: BackendKind::Cuda,
                ..
            }
        )));
    assert!(matches!(
        paths.candidates(AccelerationMode::Cuda).first(),
        Some(EngineCandidate::Worker {
            backend: BackendKind::Cuda,
            ..
        })
    ));
}

#[test]
fn readiness_starts_unloaded_and_records_model_errors() {
    let engine = SttEngine::default();
    assert_eq!(engine.readiness(), SttReadiness::Unloaded);

    let error = engine
        .ensure_loaded(
            Path::new("missing-model-for-readiness-test.bin"),
            AccelerationMode::Cpu,
            &WorkerPaths::default(),
        )
        .expect_err("missing model must fail before backend startup");

    assert!(matches!(error, AppError::ModelFileMissing));
    let public_error = fono_types::CoreError::from(&error);
    assert_eq!(
        serde_json::to_value(&public_error).unwrap()["code"],
        "model_not_found"
    );
    assert_eq!(public_error.message, "The selected model was not found");
    assert_eq!(
        engine.readiness(),
        SttReadiness::Failed {
            message: public_error.message
        }
    );
}

#[test]
fn environment_message_adapter_and_observer_preserve_the_hosts_authored_contract() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener = observed.clone();
    let engine = SttEngine::new(SttEnvironment {
        error_message: Arc::new(|error| match error {
            AppError::ModelFileMissing => "host-authored model guidance".into(),
            _ => "host-authored generic guidance".into(),
        }),
        readiness: Some(Arc::new(move |state| listener.lock().push(state))),
        ..SttEnvironment::default()
    });
    assert_eq!(observed.lock().as_slice(), &[SttReadiness::Unloaded]);
    let _ = engine.ensure_loaded(
        Path::new("missing-model-for-host-message-contract.bin"),
        AccelerationMode::Cpu,
        &WorkerPaths::default(),
    );
    let failed = SttReadiness::Failed {
        message: "host-authored model guidance".into(),
    };
    assert_eq!(engine.readiness(), failed);
    assert_eq!(
        observed.lock().as_slice(),
        &[SttReadiness::Unloaded, failed]
    );
}

#[test]
fn readiness_observer_receives_current_and_failed_states() {
    let engine = SttEngine::default();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_by_listener = Arc::clone(&observed);
    engine.set_readiness_observer(Arc::new(move |readiness| {
        observed_by_listener.lock().push(readiness);
    }));

    let _ = engine.ensure_loaded(
        Path::new("missing-model-for-readiness-observer-test.bin"),
        AccelerationMode::Cpu,
        &WorkerPaths::default(),
    );

    assert!(matches!(
        observed.lock().as_slice(),
        [SttReadiness::Unloaded, SttReadiness::Failed { .. }]
    ));
}

#[test]
fn health_is_unloaded_before_any_model_is_prepared() {
    assert_eq!(SttEngine::default().health(), SttHealth::Unloaded);
}
