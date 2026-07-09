//! Wake word subsystem for Fono.
//!
//! Provides a small, backend-agnostic API. Backends live in [`backend`] and are
//! selected through [`WakeWordConfig::backend`].

pub mod audio_source;
pub mod backend;
pub mod config;
pub mod engine;
pub mod error;
pub mod event;

pub use config::WakeWordConfig;
pub use engine::{WakeWordEngine, WakeWordHandle};
pub use error::{WakeWordError, WakeWordResult};
pub use event::{WakeWordBackend, WakeWordEvent, WakeWordStatus};
