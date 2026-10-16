use std::path::PathBuf;

pub type AudioResult<T> = Result<T, AudioError>;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("model not found: {0}")]
    ModelNotFound(PathBuf),

    #[error("failed to load model: {0}")]
    ModelLoad(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<cpal::BuildStreamError> for AudioError {
    fn from(error: cpal::BuildStreamError) -> Self {
        Self::Audio(error.to_string())
    }
}

impl From<cpal::PlayStreamError> for AudioError {
    fn from(error: cpal::PlayStreamError) -> Self {
        Self::Audio(error.to_string())
    }
}

impl From<cpal::DevicesError> for AudioError {
    fn from(error: cpal::DevicesError) -> Self {
        Self::Audio(error.to_string())
    }
}

impl From<cpal::SupportedStreamConfigsError> for AudioError {
    fn from(error: cpal::SupportedStreamConfigsError) -> Self {
        Self::Audio(error.to_string())
    }
}
