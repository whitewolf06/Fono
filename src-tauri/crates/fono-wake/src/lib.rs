//! Wake word subsystem for Fono.
//!
//! Provides a small, backend-agnostic API. Backends live in [`backend`] and are
//! selected through [`WakeWordConfig::backend`].

// Compatibility paths share fono-audio's types and process-wide hub.
pub use fono_audio::audio_source;
pub mod backend;
#[cfg(feature = "sherpa-wake")]
pub mod bench;
mod callback;
pub mod config;
mod config_validation;
pub mod diag;
pub mod engine;
pub mod error;
pub mod event;
pub mod models;
#[cfg(feature = "sherpa-wake")]
pub mod replay;
#[cfg(feature = "sherpa-wake")]
pub use fono_audio::speech_vad;
pub mod test;
#[cfg(feature = "sherpa-wake")]
pub use speech_vad::{RecordedSpeechDecision, SpeechVadDecision, StreamingSpeechVad};

pub use audio_source::{AudioCursor, AudioHub, AudioPacket, AudioSubscription};
pub use callback::WakeCallback;
pub use config::WakeWordConfig;
pub use diag::Diagnostics;
pub use engine::{validate_config, WakeWordEngine, WakeWordHandle};
pub use error::{WakeWordError, WakeWordResult};
pub use event::{WakeWordBackend, WakeWordCapabilities, WakeWordEvent, WakeWordStatus};
pub use models::{model_spec, model_version, WakeModelSpec};
#[cfg(feature = "sherpa-wake")]
pub use test::test_with_wav;
pub use test::WakeWordTestResult;

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn compatibility_hub_is_the_shared_audio_type() {
        let shared = fono_audio::AudioHub::new();
        let compatibility: AudioHub = shared.clone();
        compatibility.shutdown();
        shared.shutdown();
    }

    #[test]
    fn shared_audio_errors_preserve_the_wake_error_contract() {
        let error = WakeWordError::from(fono_audio::AudioError::Audio("capture failed".into()));
        assert!(matches!(error, WakeWordError::Audio(_)));
        assert_eq!(error.to_string(), "audio error: capture failed");
        let missing = std::path::PathBuf::from("missing.onnx");
        assert!(matches!(
            WakeWordError::from(fono_audio::AudioError::ModelNotFound(missing.clone())),
            WakeWordError::ModelNotFound(path) if path == missing
        ));
    }
}
