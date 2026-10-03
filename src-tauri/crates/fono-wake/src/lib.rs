//! Wake word subsystem for Fono.
//!
//! Provides a small, backend-agnostic API. Backends live in [`backend`] and are
//! selected through [`WakeWordConfig::backend`].

pub mod audio_source;
pub mod backend;
mod callback;
pub mod config;
mod config_validation;
pub mod diag;
pub mod engine;
pub mod error;
pub mod event;
pub mod models;
mod phrases;
#[cfg(feature = "sherpa-wake")]
pub mod replay;
#[cfg(feature = "sherpa-wake")]
pub mod speech_vad;
pub mod test;
#[cfg(feature = "sherpa-wake")]
pub use speech_vad::{SpeechVadDecision, StreamingSpeechVad};

pub use audio_source::{AudioCursor, AudioHub, AudioPacket, AudioSubscription};
#[cfg(feature = "whisper-wake")]
pub use backend::test_whisper_with_samples;
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
