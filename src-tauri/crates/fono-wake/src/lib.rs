//! Wake word subsystem for Fono.
//!
//! Provides a small, backend-agnostic API. Backends live in [`backend`] and are
//! selected through [`WakeWordConfig::backend`].

pub mod audio_source;
pub mod backend;
pub mod config;
pub mod diag;
pub mod engine;
pub mod error;
pub mod event;
pub mod test;

pub use config::WakeWordConfig;
pub use diag::Diagnostics;
pub use engine::{WakeWordEngine, WakeWordHandle};
pub use error::{WakeWordError, WakeWordResult};
pub use event::{WakeWordBackend, WakeWordEvent, WakeWordStatus};
pub use test::WakeWordTestResult;
#[cfg(feature = "sherpa-wake")]
pub use test::test_with_wav;
