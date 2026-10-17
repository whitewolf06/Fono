//! Preserved local wake calibration and validation records.

use super::WakeWordBackend;
use serde::{Deserialize, Serialize};

/// Persisted result of a completed local wake-word calibration. This is
/// descriptive metadata for FONO-45 to validate and activate later; FONO-44
/// deliberately does not alter the current wake threshold automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCalibrationProfile {
    pub backend: WakeWordBackend,
    pub model_version: String,
    pub phrase: String,
    pub graph: String,
    pub threshold: f32,
    pub sensitivity: f32,
    pub vad_threshold: f32,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub accepted_samples: u8,
    pub rejected_samples: u8,
    pub average_rms: f32,
    pub average_peak: f32,
    pub average_active_ms: u64,
    /// Present only after a separate, fresh validation session succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<WakeCalibrationValidation>,
}

/// Aggregate outcome of the post-registration validation. It deliberately
/// contains counts, not recordings, transcripts, keywords, or a made-up
/// accuracy percentage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCalibrationValidation {
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub positive_passed: u8,
    pub positive_required: u8,
    pub negative_passed: u8,
    pub negative_required: u8,
    pub confirmed_threshold: f32,
}
