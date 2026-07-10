use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeWordBackend {
    Disabled,
    WhisperExperimental,
    SherpaOnnx,
    Mock,
}

impl Default for WakeWordBackend {
    fn default() -> Self {
        WakeWordBackend::SherpaOnnx
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WakeWordStatus {
    Off,
    Loading,
    Listening,
    Processing,
    Paused,
    MissingModel,
}

impl std::fmt::Display for WakeWordStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            WakeWordStatus::Off => "off",
            WakeWordStatus::Loading => "loading",
            WakeWordStatus::Listening => "listening",
            WakeWordStatus::Processing => "processing",
            WakeWordStatus::Paused => "paused",
            WakeWordStatus::MissingModel => "missing_model",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WakeWordEvent {
    /// Engine is active and waiting for the keyword.
    Listening,
    /// Engine paused, usually while dictation is in progress.
    Paused,
    /// Wake word detected.
    Detected {
        phrase: String,
        /// Последние сэмплы до момента детекции. Используются основным
        /// конвейером как pre-roll, чтобы не терять слова сразу после wake word.
        pre_roll: Vec<i16>,
    },
    /// Non-fatal backend error.
    Error { message: String },
    /// Backend is loading its model.
    ModelLoading,
    /// Model files are missing; user must download them.
    MissingModel { path: String },
}
