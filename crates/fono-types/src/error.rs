//! Stable errors shared by recognition engines and transports.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Public machine-readable codes. Existing spellings are part of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ModelNotFound,
    ModelNotReady,
    Cancelled,
    Busy,
    InvalidRequest,
    UnsupportedAudio,
    WorkerFailed,
    Network,
    Unauthorized,
    Internal,
}

/// Transport-neutral error with an authored, safe message.
///
/// Do not put source errors, credentials, local paths or transcript text in
/// `message`. Desktop adapters still apply their own IPC message allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct CoreError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl CoreError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_error_codes_keep_their_wire_spelling() {
        for (code, spelling) in [
            (ErrorCode::ModelNotFound, "model_not_found"),
            (ErrorCode::ModelNotReady, "model_not_ready"),
            (ErrorCode::Cancelled, "cancelled"),
            (ErrorCode::Busy, "busy"),
            (ErrorCode::InvalidRequest, "invalid_request"),
            (ErrorCode::UnsupportedAudio, "unsupported_audio"),
            (ErrorCode::WorkerFailed, "worker_failed"),
            (ErrorCode::Network, "network"),
            (ErrorCode::Unauthorized, "unauthorized"),
            (ErrorCode::Internal, "internal"),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), spelling);
            assert_eq!(
                serde_json::from_value::<ErrorCode>(serde_json::json!(spelling)).unwrap(),
                code
            );
        }
        assert!(serde_json::from_str::<ErrorCode>("\"future_error\"").is_err());
    }

    #[test]
    fn core_error_preserves_public_message_and_retry_policy() {
        let error = CoreError::new(ErrorCode::Busy, "recognizer is busy", true);
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "code": "busy",
                "message": "recognizer is busy",
                "retryable": true
            })
        );
        assert_eq!(serde_json::from_value::<CoreError>(encoded).unwrap(), error);
        assert_eq!(error.to_string(), "recognizer is busy");
    }
}
