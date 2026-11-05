//! Transport-neutral speech recognition values shared by Fono applications.
//!
//! Model availability, settings, devices and application runtimes belong to
//! callers. This crate has no Tauri or native inference dependency. Decoding
//! values are the worker protocol's own types, without its inference feature.

mod acceleration;
pub mod error;
mod stt_models;
mod transcript;
pub mod transcription_contract;
mod whisper_artifacts;
mod whisper_models;

pub use acceleration::{AccelerationCapabilities, AccelerationMode};
pub use error::{CoreError, CoreResult, ErrorCode};
pub use fono_stt_protocol::{DecodingOptions, DecodingStrategy};
pub use stt_models::{ModelId, SttEngine, SttModelSelection};
pub use transcript::{RemoteBackend, RemoteTranscriptMetadata, Transcript};
pub use transcription_contract::*;
pub use whisper_artifacts::WhisperArtifact;
pub use whisper_models::WhisperModelSize;
