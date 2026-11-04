//! Recognition errors with backend metadata, independent of desktop IPC.

use fono_types::{CoreError, ErrorCode};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionBackend {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

#[derive(Debug, Error)]
pub enum SttError {
    #[error(transparent)]
    Remote(#[from] crate::remote::RemoteError),
    #[error("операция уже выполняется: {0}")]
    Busy(String),
    #[error("операция отменена: {0}")]
    Cancelled(String),
    #[error("аудио ошибка: {0}")]
    Audio(String),
    #[error("ошибка whisper/stt: {0}")]
    Stt(String),
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),
    #[error("ошибка JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ошибка конфигурации: {0}")]
    Config(String),
    #[error("модель не загружена")]
    ModelNotLoaded,
    #[error(
        "Выберите модель распознавания в настройках. Если модели ещё нет, сначала загрузите её."
    )]
    ModelNotSelected,
    #[error(
        "Файл выбранной модели не найден. Загрузите модель или выберите другой файл в настройках."
    )]
    ModelFileMissing,
    #[error("STT backend is not packaged: {0:?}")]
    BackendUnavailable(RecognitionBackend),
    #[error("STT backend failed to start: {0:?}")]
    BackendStartFailed(RecognitionBackend),
    #[error("внутренняя ошибка: {0}")]
    Internal(String),
}

impl SttError {
    /// Standalone fallback; applications can inject their own authored messages.
    pub fn user_message(&self) -> String {
        CoreError::from(self).message
    }
}

impl From<&SttError> for CoreError {
    fn from(error: &SttError) -> Self {
        if let SttError::Remote(error) = error {
            return error.core_error();
        }
        let (code, message, retryable) = match error {
            SttError::Remote(_) => unreachable!(),
            SttError::Busy(_) => (ErrorCode::Busy, "The recognizer is busy", true),
            SttError::Cancelled(_) => (ErrorCode::Cancelled, "Recognition was cancelled", false),
            SttError::Audio(_) => (
                ErrorCode::UnsupportedAudio,
                "Audio could not be processed",
                false,
            ),
            SttError::Stt(_) | SttError::BackendStartFailed(_) => {
                (ErrorCode::WorkerFailed, "Recognition failed", true)
            }
            SttError::Config(_) => (
                ErrorCode::InvalidRequest,
                "Invalid recognition configuration",
                false,
            ),
            SttError::ModelNotLoaded
            | SttError::ModelNotSelected
            | SttError::BackendUnavailable(_) => (
                ErrorCode::ModelNotReady,
                "The selected recognizer is not ready",
                true,
            ),
            SttError::ModelFileMissing => (
                ErrorCode::ModelNotFound,
                "The selected model was not found",
                false,
            ),
            SttError::Io(_) | SttError::Json(_) | SttError::Internal(_) => (
                ErrorCode::Internal,
                "Recognition could not be completed",
                false,
            ),
        };
        Self::new(code, message, retryable)
    }
}

impl From<SttError> for CoreError {
    fn from(error: SttError) -> Self {
        Self::from(&error)
    }
}

impl From<CoreError> for SttError {
    fn from(error: CoreError) -> Self {
        match error.code {
            ErrorCode::Busy => Self::Busy(error.message),
            ErrorCode::Cancelled => Self::Cancelled(error.message),
            ErrorCode::InvalidRequest => Self::Config(error.message),
            ErrorCode::UnsupportedAudio => Self::Audio(error.message),
            ErrorCode::WorkerFailed => Self::Stt(error.message),
            ErrorCode::ModelNotFound => Self::ModelFileMissing,
            ErrorCode::ModelNotReady => Self::ModelNotLoaded,
            ErrorCode::Network | ErrorCode::Unauthorized | ErrorCode::Internal => {
                Self::Internal(error.message)
            }
        }
    }
}

impl From<fono_audio::AudioError> for SttError {
    fn from(error: fono_audio::AudioError) -> Self {
        Self::Audio(error.to_string())
    }
}

impl From<anyhow::Error> for SttError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

pub type SttResult<T> = Result<T, SttError>;
// Internal aliases keep the existing algorithms and their tests easy to audit.
pub type AppError = SttError;
pub type AppResult<T> = SttResult<T>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_core_errors_never_expose_diagnostic_sources() {
        for error in [
            SttError::Stt("PRIVATE /home/user/model.bin".into()),
            SttError::Config("PRIVATE model configuration".into()),
            SttError::Io(std::io::Error::other("PRIVATE")),
            SttError::BackendStartFailed(RecognitionBackend::Cuda),
        ] {
            let encoded = serde_json::to_string(&CoreError::from(&error)).unwrap();
            assert!(!encoded.contains("PRIVATE"));
            assert!(!encoded.contains("/home/"));
        }
    }
}
