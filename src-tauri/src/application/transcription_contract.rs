//! Desktop error adaptation for the shared transcription contracts.

pub use fono_types::transcription_contract::*;

use crate::error::AppError;

impl From<AppError> for TranscriptionServiceError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Cancelled(message) => Self::Cancelled(message),
            AppError::Busy(message) => Self::Busy(message),
            AppError::ModelNotLoaded => Self::ModelNotReady("model is not loaded".into()),
            AppError::Stt(message) => Self::ModelNotReady(message),
            error @ (AppError::ModelNotSelected
            | AppError::ModelFileMissing
            | AppError::BackendUnavailable(_)
            | AppError::BackendStartFailed(_)) => Self::ModelNotReady(error.user_message()),
            other => Self::Failed(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RecognitionBackend;

    #[test]
    fn typed_native_model_errors_preserve_http_model_not_ready_shape() {
        for error in [
            AppError::ModelNotSelected,
            AppError::ModelFileMissing,
            AppError::BackendUnavailable(RecognitionBackend::Vulkan),
            AppError::BackendStartFailed(RecognitionBackend::Cpu),
        ] {
            let encoded = serde_json::to_value(TranscriptionServiceError::from(error)).unwrap();
            assert_eq!(encoded["code"], "model_not_ready");
            assert!(encoded["message"].is_string());
            assert!(encoded.get("params").is_none());
        }
    }
}
