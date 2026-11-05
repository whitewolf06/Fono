use super::*;
use crate::types::{
    ModelId, RemoteSttSettings, SttEngine, SttModelSelection, SttSource, WhisperModelSize,
};
use fono_types::RemoteTranscriptMetadata;

fn remote_settings() -> Settings {
    Settings {
        stt_source: SttSource::FonoServer,
        stt_model: Some(SttModelSelection::from_whisper_path(
            "PRIVATE/ggml-small.bin".into(),
        )),
        remote_stt: Some(RemoteSttSettings {
            url: "https://PRIVATE_SERVER.invalid".into(),
            display_name: Some("Kari".into()),
            model: ModelId::whisper(WhisperModelSize::Tiny),
            ca_pem: Some("PRIVATE_CA".into()),
            fallback: Default::default(),
        }),
        acceleration: AccelerationMode::Cuda,
        ..Settings::default()
    }
}

fn remote_transcript() -> Transcript {
    Transcript {
        text: "PRIVATE_TRANSCRIPT".into(),
        detected_language: Some("ru".into()),
        transcribe_secs: Some(99.0),
        audio_secs: Some(2.0),
        device: Some("remote:cuda:0".into()),
        remote: Some(RemoteTranscriptMetadata {
            model: ModelId::whisper(WhisperModelSize::LargeTurbo),
            backend: RemoteBackend::Cuda,
            device: "cuda:0".into(),
            request_ms: 1300,
            server_total_ms: 1100,
            queue_ms: Some(100),
            load_ms: Some(200),
            decode_ms: Some(700),
            network_ms: None,
        }),
    }
}

#[test]
fn actual_response_facts_override_requested_model_and_preserve_real_measurements() {
    let mut settings = remote_settings();
    let metadata =
        DictationHistoryMetadata::from_result(&settings, &remote_transcript(), Default::default());
    assert_eq!(metadata.backend, Some(DictationBackend::Remote));
    assert_eq!(metadata.model.as_deref(), Some("whisper:large_turbo"));
    assert_eq!(metadata.recognizer, Some(SttEngine::Whisper));
    assert_eq!(metadata.remote_name.as_deref(), Some("Kari"));
    assert_eq!(metadata.remote_device.as_deref(), Some("cuda:0"));
    assert_eq!(metadata.remote_backend, Some(RemoteBackend::Cuda));
    assert_eq!(metadata.request_duration_ms, Some(1300));
    assert_eq!(metadata.server_total_duration_ms, Some(1100));
    assert_eq!(metadata.decode_duration_ms, Some(700));
    assert_eq!(metadata.recognition_duration_ms, Some(700));
    assert!(
        metadata.network_duration_ms.is_none(),
        "request minus server total is not network-only measurement"
    );
    settings.remote_stt.as_mut().unwrap().display_name = Some("New label".into());
    settings.remote_stt.as_mut().unwrap().model = ModelId::whisper(WhisperModelSize::Base);
    assert_eq!(metadata.remote_name.as_deref(), Some("Kari"));
    assert_eq!(metadata.model.as_deref(), Some("whisper:large_turbo"));
    let encoded = serde_json::to_string(&metadata).unwrap();
    assert!(!encoded.contains("PRIVATE"));
    assert!(!encoded.contains("https://"));
    assert!(!encoded.contains("ggml-small.bin"));
    settings.stt_fallback = true;
    assert!(
        !DictationHistoryMetadata::from_result(&settings, &remote_transcript(), Default::default())
            .fallback,
        "actual remote success cannot retain a stale local-reserve marker"
    );
}

#[test]
fn absent_decode_and_network_never_reuse_legacy_total_or_invent_zero() {
    let mut transcript = remote_transcript();
    transcript.remote.as_mut().unwrap().decode_ms = None;
    let metadata =
        DictationHistoryMetadata::from_result(&remote_settings(), &transcript, Default::default());
    assert_eq!(metadata.server_total_duration_ms, Some(1100));
    assert_eq!(metadata.request_duration_ms, Some(1300));
    assert!(metadata.recognition_duration_ms.is_none());
    assert!(metadata.decode_duration_ms.is_none());
    assert!(metadata.network_duration_ms.is_none());
    transcript.remote.as_mut().unwrap().decode_ms = Some(0);
    let measured_zero =
        DictationHistoryMetadata::from_result(&remote_settings(), &transcript, Default::default());
    assert_eq!(measured_zero.decode_duration_ms, Some(0));
}

#[test]
fn local_fallback_is_local_even_with_retained_server_preferences() {
    let settings = Settings {
        stt_fallback: true,
        ..remote_settings()
    };
    let transcript = Transcript {
        device: Some("CPU".into()),
        remote: None,
        ..remote_transcript()
    };
    let metadata =
        DictationHistoryMetadata::from_result(&settings, &transcript, Default::default());
    assert_eq!(metadata.backend, Some(DictationBackend::Cpu));
    assert_eq!(metadata.model.as_deref(), Some("ggml-small.bin"));
    assert!(metadata.fallback);
    assert!(metadata.remote_name.is_none());
    assert!(metadata.remote_backend.is_none());
    assert!(metadata.remote_device.is_none());
    assert!(metadata.request_duration_ms.is_none());
    assert_eq!(metadata.recognition_duration_ms, Some(99000));
}

#[test]
fn legacy_remote_device_is_an_actual_source_marker_but_unknown_devices_are_unknown() {
    for (device, physical) in [
        ("remote:cpu", RemoteBackend::Cpu),
        ("remote:cuda", RemoteBackend::Cuda),
        ("remote:cuda:12", RemoteBackend::Cuda),
        ("remote:vulkan", RemoteBackend::Vulkan),
    ] {
        let transcript = Transcript {
            device: Some(device.into()),
            remote: None,
            ..remote_transcript()
        };
        let metadata = DictationHistoryMetadata::from_result(
            &remote_settings(),
            &transcript,
            Default::default(),
        );
        assert_eq!(metadata.backend, Some(DictationBackend::Remote));
        assert_eq!(metadata.remote_backend, Some(physical));
        assert_eq!(
            metadata.remote_device.as_deref(),
            device.strip_prefix("remote:")
        );
        assert!(metadata.recognition_duration_ms.is_none());
    }
    for device in [
        "remote:metal",
        "remote:cuda:PRIVATE",
        "remote:cuda:1234",
        "PRIVATE_GPU_NAME",
        "remote:https://private.invalid",
    ] {
        let transcript = Transcript {
            device: Some(device.into()),
            remote: None,
            ..remote_transcript()
        };
        let metadata = DictationHistoryMetadata::from_result(
            &Settings::default(),
            &transcript,
            Default::default(),
        );
        assert!(metadata.backend.is_none());
        assert!(metadata.remote_device.is_none());
    }
}

#[test]
fn malformed_remote_facts_keep_a_known_remote_source_without_fabricating_timings() {
    let mut transcript = remote_transcript();
    transcript.remote.as_mut().unwrap().decode_ms = Some(1101);
    let metadata =
        DictationHistoryMetadata::from_result(&remote_settings(), &transcript, Default::default());
    assert_eq!(metadata.backend, Some(DictationBackend::Remote));
    assert!(metadata.request_duration_ms.is_none());
    assert!(metadata.server_total_duration_ms.is_none());
    assert!(metadata.decode_duration_ms.is_none());
    assert!(metadata.recognition_duration_ms.is_none());
}

#[test]
fn unknown_optional_fields_do_not_discard_old_history_records() {
    let metadata = DictationHistoryMetadata::from_result(
        &remote_settings(),
        &remote_transcript(),
        Default::default(),
    );
    let mut value = serde_json::to_value(metadata).unwrap();
    let object = value.as_object_mut().unwrap();
    object.insert("backend".into(), serde_json::json!("future_backend"));
    object.insert("remote_backend".into(), serde_json::json!("metal"));
    object.insert(
        "remote_device".into(),
        serde_json::json!("PRIVATE/hardware/name"),
    );
    object.insert(
        "remote_name".into(),
        serde_json::json!("https://PRIVATE_SERVER.invalid"),
    );
    object.insert("request_duration_ms".into(), serde_json::json!(-1));
    object.insert(
        "server_total_duration_ms".into(),
        serde_json::json!(86_400_001),
    );
    object.insert("network_duration_ms".into(), serde_json::json!("unknown"));
    object.insert("decode_duration_ms".into(), serde_json::json!(0.5));
    let restored: DictationHistoryMetadata = serde_json::from_value(value).unwrap();
    assert_eq!(restored.backend, Some(DictationBackend::Unknown));
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["backend"],
        "unknown"
    );
    assert!(restored.remote_backend.is_none());
    assert!(restored.remote_device.is_none());
    assert!(restored.remote_name.is_none());
    assert!(restored.request_duration_ms.is_none());
    assert!(restored.server_total_duration_ms.is_none());
    assert!(restored.network_duration_ms.is_none());
    assert!(restored.decode_duration_ms.is_none());
}

#[test]
fn missing_new_fields_are_backwards_compatible_and_label_never_uses_url() {
    let mut settings = remote_settings();
    for display_name in [
        None,
        Some("https://PRIVATE_SERVER.invalid".into()),
        Some("PRIVATE/path".into()),
        Some("PRIVATE\\path".into()),
        Some("private@example.test".into()),
    ] {
        settings.remote_stt.as_mut().unwrap().display_name = display_name;
        let metadata = DictationHistoryMetadata::from_result(
            &settings,
            &remote_transcript(),
            Default::default(),
        );
        assert!(metadata.remote_name.is_none());
    }
    let mut value = serde_json::to_value(DictationHistoryMetadata::from_result(
        &Settings::default(),
        &Transcript {
            remote: None,
            ..remote_transcript()
        },
        Default::default(),
    ))
    .unwrap();
    for field in [
        "remote_name",
        "remote_device",
        "remote_backend",
        "request_duration_ms",
        "server_total_duration_ms",
        "network_duration_ms",
        "decode_duration_ms",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let restored: DictationHistoryMetadata = serde_json::from_value(value).unwrap();
    assert!(restored.remote_name.is_none());
    assert!(restored.request_duration_ms.is_none());
}
