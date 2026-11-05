//! Versioned, transport-neutral values for local transcription.

use serde::{Deserialize, Serialize};

use crate::Transcript;

pub const TRANSCRIPTION_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionRequest {
    /// Mono, signed 16-bit PCM at 16 kHz. The decoder adapter owns conversion.
    pub pcm_samples: Vec<i16>,
    /// An ISO language code or `auto`.
    pub language: String,
    /// Optional public identifier of the configured model, never its
    /// filesystem path. When omitted, the runtime selects its current model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub protocol_version: u16,
    pub text: String,
    pub detected_language: Option<String>,
    pub audio_seconds: Option<f32>,
    pub transcribe_seconds: Option<f32>,
    pub model: String,
    pub backend: Option<String>,
    /// Real recognizer timestamps on the original file timeline. Old results
    /// remain readable; missing timing never becomes fabricated subtitles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<SubtitleSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl TranscriptionResult {
    pub fn from_transcript(transcript: Transcript, model: String) -> Self {
        Self {
            protocol_version: TRANSCRIPTION_PROTOCOL_VERSION,
            text: transcript.text,
            detected_language: transcript.detected_language,
            audio_seconds: transcript.audio_secs,
            transcribe_seconds: transcript.transcribe_secs,
            model,
            backend: transcript.device,
            segments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum TranscriptionServiceError {
    InvalidRequest(String),
    Cancelled(String),
    Busy(String),
    ModelNotFound(String),
    ModelNotReady(String),
    WorkerFailed(String),
    Failed(String),
}

pub type TranscriptionServiceResult<T> = Result<T, TranscriptionServiceError>;

pub fn validate_request(request: &TranscriptionRequest) -> TranscriptionServiceResult<()> {
    if request.pcm_samples.is_empty() {
        return Err(TranscriptionServiceError::InvalidRequest(
            "PCM input must not be empty".into(),
        ));
    }
    if request.language.trim().is_empty() {
        return Err(TranscriptionServiceError::InvalidRequest(
            "language must be an ISO code or auto".into(),
        ));
    }
    if let Some(model) = &request.model {
        if model.trim().is_empty() {
            return Err(TranscriptionServiceError::InvalidRequest(
                "model identifier must not be empty".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_keeps_the_existing_optional_model_contract() {
        let legacy = serde_json::json!({"pcm_samples":[1,-2,3],"language":"auto"});
        let mut request: TranscriptionRequest = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(request.model, None);
        assert_eq!(serde_json::to_value(&request).unwrap(), legacy);
        request.model = Some("large_turbo".into());
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "pcm_samples":[1,-2,3],
                "language":"auto",
                "model":"large_turbo"
            })
        );
    }

    #[test]
    fn result_conversion_keeps_existing_fields_and_does_not_invent_timing() {
        let transcript = Transcript {
            remote: None,
            text: "Hello".into(),
            detected_language: Some("en".into()),
            transcribe_secs: Some(0.25),
            audio_secs: Some(1.5),
            device: Some("cuda".into()),
        };
        let encoded = serde_json::to_value(TranscriptionResult::from_transcript(
            transcript,
            "small".into(),
        ))
        .unwrap();
        let expected = serde_json::json!({
            "protocol_version":1,
            "text":"Hello",
            "detected_language":"en",
            "audio_seconds":1.5,
            "transcribe_seconds":0.25,
            "model":"small",
            "backend":"cuda"
        });
        assert_eq!(encoded, expected);
        let legacy: TranscriptionResult = serde_json::from_value(expected).unwrap();
        assert!(legacy.segments.is_empty());

        let mut timed = legacy;
        timed.segments.push(SubtitleSegment {
            start_ms: 500,
            end_ms: 1250,
            text: "Hello".into(),
        });
        assert_eq!(
            serde_json::to_value(&timed).unwrap()["segments"],
            serde_json::json!([{"start_ms":500,"end_ms":1250,"text":"Hello"}])
        );
        assert_eq!(
            serde_json::from_value::<TranscriptionResult>(serde_json::to_value(&timed).unwrap())
                .unwrap(),
            timed
        );
    }

    #[test]
    fn validation_retains_existing_invalid_request_errors() {
        for (samples, language, model, message) in [
            (vec![], "auto", None, "PCM input must not be empty"),
            (vec![1], " ", None, "language must be an ISO code or auto"),
            (
                vec![1],
                "auto",
                Some(" "),
                "model identifier must not be empty",
            ),
        ] {
            let request = TranscriptionRequest {
                pcm_samples: samples,
                language: language.into(),
                model: model.map(str::to_owned),
            };
            assert_eq!(
                serde_json::to_value(validate_request(&request).unwrap_err()).unwrap(),
                serde_json::json!({"code":"invalid_request","message":message})
            );
        }
        assert!(validate_request(&TranscriptionRequest {
            pcm_samples: vec![1],
            language: "auto".into(),
            model: None,
        })
        .is_ok());
    }

    #[test]
    fn legacy_service_errors_keep_the_code_message_shape() {
        for (error, code) in [
            (
                TranscriptionServiceError::InvalidRequest("message".into()),
                "invalid_request",
            ),
            (
                TranscriptionServiceError::Cancelled("message".into()),
                "cancelled",
            ),
            (TranscriptionServiceError::Busy("message".into()), "busy"),
            (
                TranscriptionServiceError::ModelNotFound("message".into()),
                "model_not_found",
            ),
            (
                TranscriptionServiceError::ModelNotReady("message".into()),
                "model_not_ready",
            ),
            (
                TranscriptionServiceError::WorkerFailed("message".into()),
                "worker_failed",
            ),
            (
                TranscriptionServiceError::Failed("message".into()),
                "failed",
            ),
        ] {
            let encoded = serde_json::to_value(&error).unwrap();
            assert_eq!(
                encoded,
                serde_json::json!({"code":code,"message":"message"})
            );
            assert_eq!(
                serde_json::from_value::<TranscriptionServiceError>(encoded).unwrap(),
                error
            );
        }
    }
}
