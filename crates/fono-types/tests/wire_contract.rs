//! Literal wire examples shared by the desktop and future server consumers.

use fono_types::{
    AccelerationMode, DecodingOptions, DecodingStrategy, ModelId, SttEngine, SttModelSelection,
    SubtitleSegment, Transcript, TranscriptionRequest, TranscriptionResult,
    TranscriptionServiceError, WhisperModelSize, TRANSCRIPTION_PROTOCOL_VERSION,
};
use serde_json::{from_str, to_value, Value};

fn json(source: &str) -> Value {
    from_str(source).expect("literal JSON fixture")
}

#[test]
fn model_identity_keeps_engine_names_and_rejects_unknown_fields() {
    for (source, engine, variant) in [
        (
            r#"{"engine":"whisper","variant":"large_turbo"}"#,
            SttEngine::Whisper,
            "large_turbo",
        ),
        (
            r#"{"engine":"giga_am","variant":"v3"}"#,
            SttEngine::GigaAm,
            "v3",
        ),
        (
            r#"{"engine":"parakeet","variant":"tdt"}"#,
            SttEngine::Parakeet,
            "tdt",
        ),
    ] {
        let id: ModelId = from_str(source).unwrap();
        assert_eq!(id.engine, engine);
        assert_eq!(id.variant, variant);
        assert_eq!(to_value(id).unwrap(), json(source));
    }
    for invalid in [
        r#"{"engine":"whisper","variant":"tiny","path":"/models/tiny.bin"}"#,
        r#"{"engine":"future_engine","variant":"tiny"}"#,
    ] {
        assert!(from_str::<ModelId>(invalid).is_err());
    }
}

#[test]
fn model_selection_preserves_its_path_and_remains_strict_at_both_levels() {
    let source =
        r#"{"id":{"engine":"whisper","variant":"tiny"},"local_path":"C:\\models\\ggml-tiny.bin"}"#;
    let selection: SttModelSelection = from_str(source).unwrap();
    assert_eq!(selection.local_path, r"C:\models\ggml-tiny.bin");
    assert_eq!(selection.id, ModelId::whisper(WhisperModelSize::Tiny));
    assert_eq!(to_value(selection).unwrap(), json(source));
    for invalid in [
        r#"{"id":{"engine":"whisper","variant":"tiny"},"local_path":"/models/ggml-tiny.bin","future":true}"#,
        r#"{"id":{"engine":"whisper","variant":"tiny","future":true},"local_path":"/models/ggml-tiny.bin"}"#,
    ] {
        assert!(from_str::<SttModelSelection>(invalid).is_err());
    }
}

#[test]
fn legacy_transcript_defaults_optional_measurements_without_changing_null_fields() {
    let transcript: Transcript = from_str(r#"{"text":"Старый результат"}"#).unwrap();
    assert_eq!(transcript.text, "Старый результат");
    assert!(transcript.detected_language.is_none());
    assert!(transcript.transcribe_secs.is_none());
    assert!(transcript.audio_secs.is_none());
    assert!(transcript.device.is_none());
    assert_eq!(
        to_value(transcript).unwrap(),
        json(
            r#"{"text":"Старый результат","detected_language":null,"transcribe_secs":null,"audio_secs":null,"device":null}"#
        )
    );
    let measured = r#"{"text":"Ready","detected_language":"en","transcribe_secs":0.5,"audio_secs":2.0,"device":"CUDA"}"#;
    assert_eq!(
        to_value(from_str::<Transcript>(measured).unwrap()).unwrap(),
        json(measured)
    );
}

#[test]
fn acceleration_and_model_size_keep_the_desktop_wire_values() {
    for (source, expected) in [
        (r#""auto""#, AccelerationMode::Auto),
        (r#""cuda""#, AccelerationMode::Cuda),
        (r#""vulkan""#, AccelerationMode::Vulkan),
        (r#""cpu""#, AccelerationMode::Cpu),
    ] {
        assert_eq!(from_str::<AccelerationMode>(source).unwrap(), expected);
        assert_eq!(to_value(expected).unwrap(), json(source));
    }
    for (source, expected) in [
        (r#""tiny""#, WhisperModelSize::Tiny),
        (r#""base""#, WhisperModelSize::Base),
        (r#""small""#, WhisperModelSize::Small),
        (r#""medium""#, WhisperModelSize::Medium),
        (r#""large""#, WhisperModelSize::Large),
        (r#""large_turbo""#, WhisperModelSize::LargeTurbo),
    ] {
        assert_eq!(from_str::<WhisperModelSize>(source).unwrap(), expected);
        assert_eq!(to_value(expected).unwrap(), json(source));
    }
    assert!(from_str::<AccelerationMode>(r#""future_gpu""#).is_err());
    assert!(from_str::<WhisperModelSize>(r#""large-v3""#).is_err());
}

#[test]
fn decoding_is_the_protocol_type_with_unchanged_legacy_defaults() {
    let legacy: DecodingOptions = from_str("{}").unwrap();
    assert_eq!(legacy, fono_stt_protocol::DecodingOptions::default());
    assert_eq!(legacy.strategy, DecodingStrategy::Fast);
    assert!(legacy.threads.is_none());
    assert_eq!(
        to_value(legacy).unwrap(),
        json(r#"{"strategy":"fast","threads":null}"#)
    );
    let source = r#"{"strategy":"accurate","threads":3}"#;
    let protocol: fono_stt_protocol::DecodingOptions = from_str(source).unwrap();
    let shared: DecodingOptions = protocol;
    let strategy: DecodingStrategy = fono_stt_protocol::DecodingStrategy::Accurate;
    assert_eq!(shared.strategy, strategy);
    assert_eq!(shared.threads, Some(3));
    assert_eq!(to_value(shared).unwrap(), json(source));
    assert!(from_str::<DecodingOptions>(r#"{"strategy":"fast","future":true}"#).is_err());
    assert!(from_str::<DecodingOptions>(r#"{"strategy":"guess"}"#).is_err());
}

#[test]
fn transcription_requests_default_and_omit_only_the_optional_model() {
    let legacy = r#"{"pcm_samples":[-32768,0,32767],"language":"auto"}"#;
    let request: TranscriptionRequest = from_str(legacy).unwrap();
    assert_eq!(request.pcm_samples, [-32768, 0, 32767]);
    assert_eq!(request.language, "auto");
    assert!(request.model.is_none());
    assert_eq!(to_value(request).unwrap(), json(legacy));
    let selected = r#"{"pcm_samples":[1],"language":"ru","model":"large_turbo"}"#;
    let request: TranscriptionRequest = from_str(selected).unwrap();
    assert_eq!(request.model.as_deref(), Some("large_turbo"));
    assert_eq!(to_value(request).unwrap(), json(selected));
}

#[test]
fn transcription_results_read_legacy_data_and_preserve_real_subtitle_fields() {
    assert_eq!(TRANSCRIPTION_PROTOCOL_VERSION, 1);
    let legacy: TranscriptionResult =
        from_str(r#"{"protocol_version":1,"text":"Ready","model":"base"}"#).unwrap();
    assert!(legacy.segments.is_empty());
    assert!(legacy.detected_language.is_none());
    assert!(legacy.audio_seconds.is_none());
    assert!(legacy.transcribe_seconds.is_none());
    assert!(legacy.backend.is_none());
    assert_eq!(
        to_value(legacy).unwrap(),
        json(
            r#"{"protocol_version":1,"text":"Ready","detected_language":null,"audio_seconds":null,"transcribe_seconds":null,"model":"base","backend":null}"#
        )
    );
    let timed = r#"{"protocol_version":1,"text":"Привет","detected_language":"ru","audio_seconds":2.0,"transcribe_seconds":0.5,"model":"tiny","backend":"CPU","segments":[{"start_ms":125,"end_ms":1500,"text":"Привет"}]}"#;
    let result: TranscriptionResult = from_str(timed).unwrap();
    assert_eq!(result.segments.len(), 1);
    assert_eq!(
        result.segments[0],
        SubtitleSegment {
            start_ms: 125,
            end_ms: 1500,
            text: "Привет".into(),
        }
    );
    assert_eq!(to_value(result).unwrap(), json(timed));
}

#[test]
fn service_errors_keep_the_legacy_code_message_contract() {
    for (error, source) in [
        (
            TranscriptionServiceError::InvalidRequest("empty audio".into()),
            r#"{"code":"invalid_request","message":"empty audio"}"#,
        ),
        (
            TranscriptionServiceError::Cancelled("cancelled".into()),
            r#"{"code":"cancelled","message":"cancelled"}"#,
        ),
        (
            TranscriptionServiceError::Busy("busy".into()),
            r#"{"code":"busy","message":"busy"}"#,
        ),
        (
            TranscriptionServiceError::ModelNotReady("not loaded".into()),
            r#"{"code":"model_not_ready","message":"not loaded"}"#,
        ),
        (
            TranscriptionServiceError::Failed("failed".into()),
            r#"{"code":"failed","message":"failed"}"#,
        ),
    ] {
        assert_eq!(
            from_str::<TranscriptionServiceError>(source).unwrap(),
            error
        );
        assert_eq!(to_value(error).unwrap(), json(source));
    }
}
