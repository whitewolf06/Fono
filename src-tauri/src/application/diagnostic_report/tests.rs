use super::{model::*, render::render, telemetry::ReportOutcome};
use crate::types::{AccelerationMode, AiMode, InjectionMode, PipelineState, Settings};
use crate::types::{WakeWordBackend, WakeWordStatus};

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
        recognition_source: crate::types::SttSource::Local,
        model_available: Some(true),
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
fn remote_connection_address_name_and_ca_are_not_exported() {
    let settings = Settings {
        stt_source: crate::types::SttSource::FonoServer,
        remote_stt: Some(crate::types::RemoteSttSettings {
            url: "https://PRIVATE_SERVER.invalid".into(),
            display_name: Some("PRIVATE_SERVER_NAME".into()),
            ca_pem: Some("PRIVATE_CA".into()),
            model: crate::types::ModelId::whisper(crate::types::WhisperModelSize::LargeTurbo),
            fallback: Default::default(),
        }),
        ..Settings::default()
    };
    let mut data = snapshot();
    data.recognition_source = settings.stt_source;
    data.model = model_label(&settings);
    data.model_available = model_availability(&settings, |_| {
        panic!("remote report must not inspect local weights")
    });
    data.active_backend = backend_label("remote:cuda:0");
    let text = render(&data, None);
    assert!(!text.contains("PRIVATE"));
    assert!(!text.contains("https://"));
    assert!(text.contains("Модель: large_turbo"));
    assert!(text.contains("на сервере не проверялась"));
    assert!(text.contains("Fono Server (CUDA)"));
    assert!(text.contains("Локальное ускорение: не применяется"));
}

#[test]
fn remote_report_model_and_backend_are_closed_allowlists() {
    let mut settings = Settings {
        stt_source: crate::types::SttSource::FonoServer,
        whisper_model_path: Some("PRIVATE/ggml-small.bin".into()),
        remote_stt: Some(crate::types::RemoteSttSettings {
            url: "https://PRIVATE.invalid".into(),
            display_name: Some("PRIVATE_NAME".into()),
            model: crate::types::ModelId {
                engine: crate::types::SttEngine::Whisper,
                variant: "PRIVATE_MODEL".into(),
            },
            ca_pem: None,
            fallback: Default::default(),
        }),
        ..Settings::default()
    };
    assert_eq!(model_label(&settings), "другая модель сервера");
    for device in [
        "remote:cpu",
        "remote:vulkan",
        "remote:cuda",
        "remote:cuda:17",
    ] {
        assert!(backend_label(device).starts_with("Fono Server"));
    }
    for device in [
        "remote:PRIVATE_DEVICE",
        "remote:cuda:PRIVATE",
        "remote:cuda:1234",
        "remote:cuda:-1",
    ] {
        assert_eq!(backend_label(device), "неизвестно");
    }
    assert_eq!(
        model_availability(&settings, |_| panic!("no remote filesystem check")),
        None
    );
    settings.stt_source = crate::types::SttSource::Local;
    assert_eq!(model_label(&settings), "small");
    assert_eq!(model_availability(&settings, |_| true), Some(true));
    assert_eq!(
        model_availability(&Settings::default(), |_| panic!("no selection")),
        Some(false)
    );
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
    let mut last = super::telemetry::DictationMeasurement {
        operation: 7,
        source: fono_core::OperationSource::Hotkey,
        outcome: ReportOutcome::InsertionError,
        audio_ms: Some(10_000),
        capture_dropped_packets: Some(5),
        finish_ms: 2500,
        capture_finish_ms: Some(100),
        model_load_ms: Some(1000),
        recognition_ms: Some(1300),
        processing_ms: None,
        insertion_ms: Some(100),
        recognition: None,
    };
    let text = render(&snapshot(), Some(&last));
    assert!(text.contains("Длительность захваченного аудио: 10000 мс"));
    assert!(text.contains("Потери пакетов захвата в этой диктовке: 5"));
    assert!(text.contains("После команды завершения до результата: 2500 мс"));
    assert!(text.contains("Ожидание и загрузка модели: 1000 мс"));
    assert!(text.contains("Ожидание и распознавание: 1300 мс"));
    assert!(text.contains("Обработка текста: не измерено"));
    assert!(text.contains("Результат: ошибка вставки"));
    last.capture_dropped_packets = None;
    let unknown = render(&snapshot(), Some(&last));
    assert!(unknown.contains("Потери пакетов захвата в этой диктовке: не измерено"));
}

#[test]
fn per_result_remote_measurements_export_only_actual_numbers_and_closed_backend() {
    use super::telemetry::{DictationMeasurement, RecognitionMeasurement};
    let mut transcript = crate::types::Transcript {
        text: "PRIVATE_TRANSCRIPT sk-PRIVATE_TOKEN".into(),
        detected_language: Some("PRIVATE_LANGUAGE".into()),
        transcribe_secs: Some(900.0),
        audio_secs: Some(1.0),
        device: Some("PRIVATE_DEVICE_NAME".into()),
        remote: Some(fono_types::RemoteTranscriptMetadata {
            model: crate::types::ModelId {
                engine: crate::types::SttEngine::Whisper,
                variant: "PRIVATE_MODEL".into(),
            },
            backend: fono_types::RemoteBackend::Cuda,
            device: "cuda:0".into(),
            request_ms: 90,
            server_total_ms: 60,
            queue_ms: None,
            load_ms: None,
            decode_ms: None,
            network_ms: None,
        }),
    };
    let mut last = DictationMeasurement {
        operation: 8,
        source: fono_core::OperationSource::Hotkey,
        outcome: ReportOutcome::Completed,
        audio_ms: Some(1000),
        capture_dropped_packets: None,
        finish_ms: 100,
        capture_finish_ms: None,
        model_load_ms: None,
        recognition_ms: Some(90),
        processing_ms: None,
        insertion_ms: None,
        recognition: Some(RecognitionMeasurement::from_transcript(&transcript, false)),
    };
    let text = render(&snapshot(), Some(&last));
    assert!(text.contains("Фактический backend: Fono Server"));
    assert!(text.contains("Запрос к серверу целиком: 90 мс"));
    assert!(text.contains("Общее время на сервере: 60 мс"));
    assert!(text.contains("Сеть отдельно: не измерено"));
    assert!(text.contains("Распознавание на сервере отдельно: не измерено"));
    assert!(!text.contains("PRIVATE"));
    assert!(!format!("{:?}", last.recognition).contains("PRIVATE"));
    transcript.remote.as_mut().unwrap().decode_ms = Some(0);
    last.recognition = Some(RecognitionMeasurement::from_transcript(&transcript, false));
    assert!(render(&snapshot(), Some(&last)).contains("Распознавание на сервере отдельно: 0 мс"));
    // A CPU reserve result cannot inherit the preceding server's timing facts.
    transcript.remote = None;
    transcript.device = Some("CPU".into());
    last.recognition = Some(RecognitionMeasurement::from_transcript(&transcript, true));
    let fallback = render(&snapshot(), Some(&last));
    assert!(fallback.contains("Фактический backend: CPU | Локальный резерв: да"));
    assert!(!fallback.contains("Запрос к серверу целиком"));
}
