mod mock;
pub use mock::MockBackend;

#[cfg(feature = "whisper-wake")]
mod whisper_experimental;
#[cfg(feature = "whisper-wake")]
pub use whisper_experimental::WhisperExperimentalBackend;

#[cfg(feature = "sherpa-wake")]
pub(crate) mod sherpa_onnx;
#[cfg(feature = "sherpa-wake")]
pub use sherpa_onnx::SherpaOnnxBackend;
