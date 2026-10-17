use super::*;
use parking_lot::Mutex;

#[test]
fn auto_prefers_cuda_worker_before_vulkan_worker() {
    let paths = WorkerPaths::for_test_workers(
        Some(PathBuf::from("cuda.exe")),
        Some(PathBuf::from("vulkan.exe")),
    );
    let candidates = paths.candidates(AccelerationMode::Auto);
    assert!(matches!(
        candidates.first(),
        Some(EngineCandidate::Worker {
            backend: BackendKind::Cuda,
            ..
        })
    ));
    assert!(matches!(
        candidates.get(1),
        Some(EngineCandidate::Worker {
            backend: BackendKind::Vulkan,
            ..
        })
    ));
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
fn readiness_starts_unloaded_and_records_model_errors() {
    let engine = SttEngine::new();
    assert_eq!(engine.readiness(), SttReadiness::Unloaded);

    let error = engine
        .ensure_loaded(
            Path::new("missing-model-for-readiness-test.bin"),
            AccelerationMode::Cpu,
            &WorkerPaths::default(),
        )
        .expect_err("missing model must fail before backend startup");

    assert!(matches!(error, AppError::ModelFileMissing));
    assert_eq!(
        serde_json::to_value(&error).unwrap()["code"],
        "model_missing"
    );
    assert!(matches!(
        engine.readiness(),
        SttReadiness::Failed { message } if message.contains("Файл выбранной модели не найден")
            && !message.contains("missing-model-for-readiness-test.bin")
    ));
}

#[test]
fn readiness_observer_receives_current_and_failed_states() {
    let engine = SttEngine::new();
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
    assert_eq!(SttEngine::new().health(), SttHealth::Unloaded);
}
