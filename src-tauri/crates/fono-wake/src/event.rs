use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeWordBackend {
    Disabled,
    WhisperExperimental,
    #[default]
    SherpaOnnx,
    Mock,
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

/// Declares what a selected wake backend can reliably recognise.
#[derive(Debug, Clone, Serialize)]
pub struct WakeWordCapabilities {
    pub backend: WakeWordBackend,
    pub supports_custom_phrase: bool,
    pub supported_phrases: Vec<String>,
    pub includes_pre_roll: bool,
}

pub fn capabilities_for_backend(backend: WakeWordBackend) -> WakeWordCapabilities {
    match backend {
        WakeWordBackend::Disabled => WakeWordCapabilities {
            backend,
            supports_custom_phrase: false,
            supported_phrases: Vec::new(),
            includes_pre_roll: false,
        },
        WakeWordBackend::WhisperExperimental => WakeWordCapabilities {
            backend,
            supports_custom_phrase: true,
            supported_phrases: Vec::new(),
            includes_pre_roll: true,
        },
        WakeWordBackend::SherpaOnnx => WakeWordCapabilities {
            backend,
            supports_custom_phrase: false,
            supported_phrases: crate::phrases::SHERPA_SUPPORTED_PHRASES
                .iter()
                .map(|phrase| (*phrase).to_string())
                .collect(),
            includes_pre_roll: false,
        },
        WakeWordBackend::Mock => WakeWordCapabilities {
            backend,
            supports_custom_phrase: true,
            supported_phrases: Vec::new(),
            includes_pre_roll: false,
        },
    }
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

#[cfg(test)]
mod tests {
    use super::{capabilities_for_backend, WakeWordBackend};

    #[test]
    fn sherpa_advertises_only_bundled_phrases() {
        let capabilities = capabilities_for_backend(WakeWordBackend::SherpaOnnx);
        assert!(!capabilities.supports_custom_phrase);
        assert_eq!(
            capabilities.supported_phrases,
            ["hey fono", "okay fun", "рамзи"]
        );
        assert!(!capabilities.includes_pre_roll);
    }

    #[test]
    fn whisper_advertises_custom_phrase_and_pre_roll() {
        let capabilities = capabilities_for_backend(WakeWordBackend::WhisperExperimental);
        assert!(capabilities.supports_custom_phrase);
        assert!(capabilities.supported_phrases.is_empty());
        assert!(capabilities.includes_pre_roll);
    }
}
