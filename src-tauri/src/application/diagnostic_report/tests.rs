use super::{model::*, render::render, telemetry::ReportOutcome};
use crate::types::{AccelerationMode, AiMode, InjectionMode, PipelineState, Settings};
use fono_wake::{WakeWordBackend, WakeWordStatus};

fn snapshot() -> ReportSnapshot {
    ReportSnapshot {
        version: safe_version("0.5.21"),
        revision: safe_revision("595b051123ab"),
        profile: "release",
        os: "windows",
        arch: "x86_64",
        phase: PipelineState::Idle,
        paused: false,
        model: "large_turbo",
        model_available: true,
        language: "русский",
        acceleration: AccelerationMode::Auto,
        active_backend: "CUDA",
        stt_state: "готов",
        cuda_available: true,
        vulkan_available: true,
        microphone_default: false,
        microphone_available: Some(false),
        input_count: Some(2),
        capture_dropped: 3,
        subscriber_dropped: 4,
        audio_error: true,
        wake_enabled: false,
        wake_backend: WakeWordBackend::SherpaStreamingRu,
        wake_status: WakeWordStatus::Off,
        wake_calibrated: false,
        processing: AiMode::Clean,
        processing_connection: Some(crate::types::LlmConnectionKind::Local),
        processing_model_configured: false,
        dictionary_enabled: false,
        dictionary_count: 0,
        injection: InjectionMode::SendInput,
        history_enabled: true,
        trainer_enabled: false,
        verbose_logging: false,
        service_enabled: true,
        service_error: false,
        queue: [2, 0, 1, 4, 1, 0],
    }
}

#[test]
fn custom_model_paths_and_names_are_not_exported() {
    let settings = Settings {
        whisper_model_path: Some(r"C:\Users\PRIVATE_USER\PRIVATE_MODEL.bin".into()),
        ..Settings::default()
    };
    let mut data = snapshot();
    data.model = model_label(&settings);
    let text = render(&data, None);
    assert!(text.contains("пользовательская модель"));
    assert!(!text.contains("PRIVATE"));
    assert!(!text.contains(r"C:\"));
}

#[test]
fn only_public_catalog_model_ids_are_exported() {
    let settings = Settings {
        whisper_model_path: Some("ggml-large-v3-turbo.bin".into()),
        ..Settings::default()
    };
    assert_eq!(model_label(&settings), "large_turbo");
    assert_eq!(model_label(&Settings::default()), "не выбрана");
}

#[test]
fn build_and_backend_values_use_closed_allowlists() {
    for unsafe_value in [
        "sk-private-key",
        "main/private-user",
        "1.2.3\nTOKEN=SECRET",
        r"C:\Users\someone",
    ] {
        assert_eq!(safe_revision(unsafe_value), "unknown");
        assert_eq!(safe_version(unsafe_value), "unknown");
        assert_eq!(backend_label(unsafe_value), "неизвестно");
        assert_eq!(language_label(unsafe_value), "другой язык");
    }
    assert_eq!(safe_version("0.5.21"), "0.5.21");
    assert_eq!(safe_revision("ABC123DEF"), "abc123def");
    assert_eq!(backend_label("Vulkan"), "Vulkan");
}

#[test]
fn error_category_does_not_disclose_payload() {
    let error = crate::error::AppError::Stt("PRIVATE_TRANSCRIPT C:\\PRIVATE_USER sk-SECRET".into());
    let category = ReportOutcome::from_error(&error);
    assert_eq!(category, ReportOutcome::RecognitionError);
    assert_eq!(category.label(), "ошибка распознавания");
}

#[test]
fn absent_measurements_are_explicit_and_report_is_stable() {
    let first = render(&snapshot(), None);
    assert_eq!(first, render(&snapshot(), None));
    assert!(first.contains("Нет измерений"));
    assert!(first.contains("захват 3; подписчики 4"));
    assert!(first.contains("ожидают 2; подготовка 0; распознавание 1"));
    assert!(first.contains("Выбор микрофона: выбранный вручную | Доступен: нет"));
    assert!(!first.contains("http"));
    assert!(!first.contains("17832"));
}

#[test]
fn report_distinguishes_audio_duration_from_finish_and_stage_latencies() {
    let last = super::telemetry::DictationMeasurement {
        operation: 7,
        source: crate::operation::OperationSource::Hotkey,
        outcome: ReportOutcome::InsertionError,
        audio_ms: Some(10_000),
        finish_ms: 2500,
        capture_finish_ms: Some(100),
        model_load_ms: Some(1000),
        recognition_ms: Some(1300),
        processing_ms: None,
        insertion_ms: Some(100),
    };
    let text = render(&snapshot(), Some(&last));
    assert!(text.contains("Длительность захваченного аудио: 10000 мс"));
    assert!(text.contains("После команды завершения до результата: 2500 мс"));
    assert!(text.contains("Ожидание и загрузка модели: 1000 мс"));
    assert!(text.contains("Ожидание и распознавание: 1300 мс"));
    assert!(text.contains("Обработка текста: не измерено"));
    assert!(text.contains("Результат: ошибка вставки"));
}
