//! Versioned, transport-neutral values for local transcription.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::types::Transcript;

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
}

impl TranscriptionResult {
    pub(crate) fn from_transcript(transcript: Transcript, model: String) -> Self {
        Self {
            protocol_version: TRANSCRIPTION_PROTOCOL_VERSION,
            text: transcript.text,
            detected_language: transcript.detected_language,
            audio_seconds: transcript.audio_secs,
            transcribe_seconds: transcript.transcribe_secs,
            model,
            backend: transcript.device,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum TranscriptionServiceError {
    InvalidRequest(String),
    Cancelled(String),
    Busy(String),
    ModelNotReady(String),
    Failed(String),
}

impl From<AppError> for TranscriptionServiceError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Cancelled(message) => Self::Cancelled(message),
            AppError::Busy(message) => Self::Busy(message),
            AppError::ModelNotLoaded => Self::ModelNotReady("model is not loaded".into()),
            AppError::Stt(message) => Self::ModelNotReady(message),
            other => Self::Failed(other.to_string()),
        }
    }
}

pub type TranscriptionServiceResult<T> = Result<T, TranscriptionServiceError>;

pub(crate) fn validate_request(request: &TranscriptionRequest) -> TranscriptionServiceResult<()> {
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
