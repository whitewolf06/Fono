mod mock;
pub use mock::MockBackend;
#[cfg(feature = "sherpa-wake")]
mod audio_recovery;
#[cfg(feature = "sherpa-wake")]
pub(crate) mod decoder_session;
pub(crate) mod phrase_matcher;
#[cfg(feature = "sherpa-wake")]
pub(crate) mod sherpa_detector;
#[cfg(feature = "sherpa-wake")]
mod sherpa_runtime;

#[cfg(feature = "whisper-wake")]
mod whisper_experimental;
#[cfg(feature = "whisper-wake")]
mod whisper_helpers;
#[cfg(feature = "whisper-wake")]
pub use whisper_experimental::test_with_samples as test_whisper_with_samples;
#[cfg(feature = "whisper-wake")]
pub use whisper_experimental::WhisperExperimentalBackend;

#[cfg(feature = "sherpa-wake")]
pub(crate) mod sherpa_onnx;
#[cfg(feature = "sherpa-wake")]
pub use sherpa_onnx::SherpaOnnxBackend;
