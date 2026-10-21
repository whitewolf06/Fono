//! Process-wide audio capture and neural speech detection, independent of wake word.
//!
//! Create one [`AudioHub`] in the application's composition root and share its
//! clones between consumers. The optional speech VAD uses the same shared
//! sherpa-onnx dependency as other inference consumers.

pub mod audio_source;
pub mod error;
pub mod speech_boundaries;
pub mod speech_segments;
pub mod speech_timing;
pub use speech_boundaries::{recorded_speech_bounds, RecordedSpeechDecision, SpeechBounds};
pub use speech_timing::SpeechTimingSamples;
#[cfg(feature = "speech-vad")]
pub mod speech_vad;

pub use audio_source::{
    AudioCursor, AudioHub, AudioHubDiagnostics, AudioPacket, AudioStream, AudioSubscription,
};
pub use error::{AudioError, AudioResult};
#[cfg(feature = "speech-vad")]
pub use speech_vad::{SpeechVadDecision, StreamingSpeechVad};
