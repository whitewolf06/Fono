//! Persisted wake settings remain readable when the detector is not compiled.
#[cfg(feature = "wake-word")]
pub use fono_wake::{WakeWordBackend, WakeWordStatus};

#[cfg(not(feature = "wake-word"))]
mod frozen {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum WakeWordBackend {
        Disabled,
        WhisperExperimental,
        #[default]
        SherpaOnnx,
        SherpaStreamingRu,
        SherpaStreamingEn,
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
        Error,
    }
}

#[cfg(not(feature = "wake-word"))]
pub use frozen::{WakeWordBackend, WakeWordStatus};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_saved_wake_backend_keeps_its_serialized_identifier() {
        for (value, identifier) in [
            (WakeWordBackend::Disabled, "disabled"),
            (WakeWordBackend::WhisperExperimental, "whisper_experimental"),
            (WakeWordBackend::SherpaOnnx, "sherpa_onnx"),
            (WakeWordBackend::SherpaStreamingRu, "sherpa_streaming_ru"),
            (WakeWordBackend::SherpaStreamingEn, "sherpa_streaming_en"),
            (WakeWordBackend::Mock, "mock"),
        ] {
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(json, identifier);
            assert_eq!(
                serde_json::from_value::<WakeWordBackend>(json).unwrap(),
                value
            );
        }
    }
}
