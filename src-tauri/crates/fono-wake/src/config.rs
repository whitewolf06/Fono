pub use crate::event::WakeWordBackend;

#[derive(Debug, Clone)]
pub struct WakeWordConfig {
    /// Is wake word active at all?
    pub enabled: bool,
    /// Which backend implementation to use.
    pub backend: WakeWordBackend,
    /// Target wake phrase. Backends may ignore it if they use a fixed phrase.
    pub phrase: String,
    /// Directory that holds backend-specific model files.
    pub model_dir: std::path::PathBuf,
    /// Detection threshold; semantics are backend-specific.
    pub threshold: f32,
    /// Sensitivity / false-positive tuning; semantics are backend-specific.
    pub sensitivity: f32,
    /// RMS level that starts a fresh keyword-spotting speech session.
    pub vad_threshold: f32,
    /// Use the GPU when the selected backend supports it.
    pub use_gpu: bool,
    /// Required audio sample rate in Hz.
    pub sample_rate: u32,
    /// Minimum time between detections in milliseconds.
    pub cooldown_ms: u64,
    /// Optional CPAL input device id.
    pub audio_device_id: Option<String>,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: WakeWordBackend::default(),
            phrase: "hey fono".into(),
            model_dir: std::path::PathBuf::new(),
            threshold: 0.5,
            sensitivity: 0.5,
            vad_threshold: 0.015,
            use_gpu: false,
            sample_rate: 16_000,
            cooldown_ms: 2_000,
            audio_device_id: None,
        }
    }
}
