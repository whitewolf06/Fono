use super::*;
use crate::types::{ModelId, RemoteFallback, SttModelSelection, WhisperModelSize};
use std::cell::Cell;

fn settings() -> Settings {
    Settings {
        stt_source: SttSource::FonoServer,
        stt_model: Some(SttModelSelection::from_whisper_path(
            "PRIVATE_LOCAL/ggml-tiny.bin".into(),
        )),
        remote_stt: Some(RemoteSttSettings {
            url: "https://PRIVATE_SERVER.invalid".into(),
            display_name: Some("fixture server".into()),
            model: ModelId::whisper(WhisperModelSize::LargeTurbo),
            ca_pem: Some("PRIVATE_CA".into()),
            fallback: RemoteFallback::Local(ModelId::whisper(WhisperModelSize::Tiny)),
        }),
        ..Settings::default()
    }
}

fn request(model: Option<&str>) -> TranscriptionRequest {
    TranscriptionRequest {
        pcm_samples: vec![2, -3, 5, -7],
        language: "ru".into(),
        model: model.map(str::to_owned),
    }
}

fn result(model: &str, backend: &str) -> TranscriptionResult {
    TranscriptionResult {
        protocol_version: fono_types::TRANSCRIPTION_PROTOCOL_VERSION,
        text: "fixture result".into(),
        detected_language: Some("ru".into()),
        audio_seconds: Some(0.25),
        transcribe_seconds: None,
        model: model.into(),
        backend: Some(backend.into()),
        segments: Vec::new(),
    }
}

#[test]
fn remote_file_uses_selected_server_even_with_a_saved_local_model_and_reserve() {
    let settings = settings();
    let selected = settings.remote_stt.clone().unwrap();
    let request = request(Some("large_turbo"));
    let expected_request = request.clone();
    let expected_result = result("whisper:large_turbo", "remote:cuda:0");
    let actual = route(
        settings,
        request,
        OperationCancellation::default(),
        |_, _| panic!("a remote file must not reactivate saved local weights"),
        |remote, request, cancellation| {
            assert_eq!(remote, selected);
            assert_eq!(request, expected_request);
            assert!(!cancellation.is_cancelled());
            Ok(expected_result.clone())
        },
    )
    .unwrap();
    assert_eq!(actual, expected_result);
}

#[test]
fn local_file_preserves_the_local_request_path_despite_saved_remote_preferences() {
    let settings = Settings {
        stt_source: SttSource::Local,
        ..settings()
    };
    let request = request(Some("whisper:tiny"));
    let expected_request = request.clone();
    let expected_result = result("whisper:tiny", "CPU");
    let actual = route(
        settings,
        request,
        OperationCancellation::default(),
        |request, cancellation| {
            assert_eq!(request, expected_request);
            assert!(!cancellation.is_cancelled());
            Ok(expected_result.clone())
        },
        |_, _, _| panic!("saved remote preferences must not change a local file route"),
    )
    .unwrap();
    assert_eq!(actual, expected_result);
}

#[test]
fn missing_server_configuration_does_not_fall_back_or_open_a_connection() {
    let settings = Settings {
        remote_stt: None,
        ..settings()
    };
    let actual = route(
        settings,
        request(None),
        OperationCancellation::default(),
        |_, _| panic!("missing server settings must not cause local fallback"),
        |_, _, _| panic!("missing settings must fail before connection work"),
    );
    assert!(matches!(
        actual,
        Err(TranscriptionServiceError::ModelNotReady(_))
    ));
}

#[test]
fn mismatched_file_model_fails_before_either_dispatch_callback() {
    for model in ["whisper:tiny", "", "PRIVATE_LOCAL/ggml-tiny.bin"] {
        let actual = route(
            settings(),
            request(Some(model)),
            OperationCancellation::default(),
            |_, _| panic!("a model mismatch must not reach local STT"),
            |_, _, _| panic!("a model mismatch must not upload audio"),
        );
        assert!(matches!(
            actual,
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
    }
}

#[test]
fn cancellation_before_dispatch_never_enters_connection_or_credential_work() {
    let cancellation = OperationCancellation::default();
    cancellation.cancel();
    let called = Cell::new(false);
    let actual = route(
        settings(),
        request(None),
        cancellation,
        |_, _| {
            called.set(true);
            panic!("a cancelled request must not dispatch local work")
        },
        |_, _, _| {
            called.set(true);
            panic!("credential lookup and connection belong after this callback boundary")
        },
    );
    assert!(matches!(
        actual,
        Err(TranscriptionServiceError::Cancelled(_))
    ));
    assert!(!called.get());
}

#[test]
fn admitted_remote_settings_survive_edits_to_later_preferences() {
    let admitted = settings();
    let selected = admitted.remote_stt.clone().unwrap();
    let mut later = admitted.clone();
    let actual = route(
        admitted,
        request(None),
        OperationCancellation::default(),
        |_, _| panic!("the admitted remote source must stay remote"),
        |remote, request, _| {
            later.stt_source = SttSource::Local;
            let edited = later.remote_stt.as_mut().unwrap();
            edited.url = "https://ANOTHER_SERVER.invalid".into();
            edited.model = ModelId::whisper(WhisperModelSize::Small);
            edited.ca_pem = None;
            edited.fallback = RemoteFallback::Ask;
            assert_eq!(remote, selected);
            assert_eq!(request.model, None);
            Ok(result(&remote.model.public_id(), "remote:cuda:0"))
        },
    )
    .unwrap();
    assert_eq!(actual.model, selected.model.public_id());
    assert_eq!(later.stt_source, SttSource::Local);
    assert_ne!(later.remote_stt.unwrap(), selected);
}

#[test]
fn remote_file_failure_does_not_apply_the_dictation_reserve_policy() {
    for fallback in [
        RemoteFallback::None,
        RemoteFallback::Ask,
        RemoteFallback::Local(ModelId::whisper(WhisperModelSize::Tiny)),
    ] {
        let mut settings = settings();
        settings.remote_stt.as_mut().unwrap().fallback = fallback;
        let calls = Cell::new(0);
        let actual = route(
            settings,
            request(None),
            OperationCancellation::default(),
            |_, _| panic!("a server file failure must not silently load the CPU reserve"),
            |_, _, _| {
                calls.set(calls.get() + 1);
                Err(TranscriptionServiceError::WorkerFailed(
                    "fixture failure".into(),
                ))
            },
        );
        assert!(matches!(
            actual,
            Err(TranscriptionServiceError::WorkerFailed(_))
        ));
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn file_errors_keep_classification_without_exposing_source_messages() {
    use fono_stt::{
        error::SttError,
        remote::{RemoteError, RemoteErrorKind},
    };
    let remote = |kind| {
        SttError::Remote(RemoteError {
            kind,
            status: Some(503),
            retry_after_seconds: Some(1),
        })
    };
    for (error, code) in [
        (SttError::Cancelled("PRIVATE_CANCEL".into()), "cancelled"),
        (SttError::Busy("PRIVATE_BUSY".into()), "busy"),
        (SttError::Audio("PRIVATE_AUDIO".into()), "invalid_request"),
        (SttError::Config("PRIVATE_CONFIG".into()), "invalid_request"),
        (SttError::ModelFileMissing, "model_not_found"),
        (SttError::ModelNotLoaded, "model_not_ready"),
        (SttError::Stt("PRIVATE_TRANSCRIPT".into()), "worker_failed"),
        (SttError::Internal("PRIVATE_SERVER_TOKEN".into()), "failed"),
        (remote(RemoteErrorKind::Busy), "busy"),
        (remote(RemoteErrorKind::Incompatible), "invalid_request"),
        (
            remote(RemoteErrorKind::UnsupportedCapabilities),
            "invalid_request",
        ),
        (remote(RemoteErrorKind::ModelNotFound), "model_not_found"),
        (remote(RemoteErrorKind::ModelNotReady), "model_not_ready"),
        (remote(RemoteErrorKind::UnsupportedAudio), "invalid_request"),
        (remote(RemoteErrorKind::WorkerFailed), "worker_failed"),
        (remote(RemoteErrorKind::Network), "failed"),
        (remote(RemoteErrorKind::Unauthorized), "failed"),
    ] {
        let encoded = serde_json::to_value(remote_error(error)).unwrap();
        assert_eq!(encoded["code"], code);
        assert!(!encoded.to_string().contains("PRIVATE_"));
    }
}

#[test]
fn incompatible_file_servers_keep_the_update_guidance() {
    use fono_stt::remote::RemoteErrorKind;
    for kind in [
        RemoteErrorKind::Incompatible,
        RemoteErrorKind::UnsupportedCapabilities,
    ] {
        let error = remote_error(fono_stt::error::SttError::Remote(
            fono_stt::remote::RemoteError {
                kind,
                status: None,
                retry_after_seconds: None,
            },
        ));
        let encoded = serde_json::to_value(error).unwrap();
        assert_eq!(encoded["code"], "invalid_request");
        assert!(encoded["message"].as_str().unwrap().contains("Обновите"));
    }
}
