use std::path::PathBuf;

use crate::event::WakeWordBackend;

pub type WakeWordResult<T> = Result<T, WakeWordError>;

#[derive(Debug, thiserror::Error)]
pub enum WakeWordError {
    #[error("backend {0:?} was not compiled into this build")]
    BackendNotCompiled(WakeWordBackend),

    #[error("model not found: {0}")]
    ModelNotFound(PathBuf),

    #[error("failed to load model: {0}")]
    ModelLoad(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<cpal::BuildStreamError> for WakeWordError {
    fn from(e: cpal::BuildStreamError) -> Self {
        WakeWordError::Audio(e.to_string())
    }
}

impl From<cpal::PlayStreamError> for WakeWordError {
    fn from(e: cpal::PlayStreamError) -> Self {
        WakeWordError::Audio(e.to_string())
    }
}

impl From<cpal::DevicesError> for WakeWordError {
    fn from(e: cpal::DevicesError) -> Self {
        WakeWordError::Audio(e.to_string())
    }
}

impl From<cpal::SupportedStreamConfigsError> for WakeWordError {
    fn from(e: cpal::SupportedStreamConfigsError) -> Self {
        WakeWordError::Audio(e.to_string())
    }
}
