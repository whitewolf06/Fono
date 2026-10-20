//! Numeric capture-worker state, shared by the existing snapshot and meter.
use serde::Serialize;

pub const START_AFTER_MS: u64 = 15_000;
#[cfg(feature = "speech-vad")]
pub const START_AFTER_SAMPLES: usize = 15 * 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Phase {
    Inactive,
    #[cfg(feature = "speech-vad")]
    Waiting,
    #[cfg(feature = "speech-vad")]
    Listening,
    #[cfg(feature = "speech-vad")]
    Queued,
    #[cfg(feature = "speech-vad")]
    Recognizing,
    #[cfg(feature = "speech-vad")]
    Fallback,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Status {
    pub operation_id: u64,
    pub available: bool,
    pub phase: Phase,
    pub completed_segments: usize,
    pub start_after_ms: u64,
}

impl Status {
    pub(super) fn inactive(operation_id: u64) -> Self {
        Self {
            operation_id,
            available: cfg!(feature = "speech-vad"),
            phase: Phase::Inactive,
            completed_segments: 0,
            start_after_ms: START_AFTER_MS,
        }
    }
}

impl super::Pipeline {
    pub fn pretranscription_status_for(&self, operation: u64) -> Status {
        let recording = self.recording.lock();
        if !*recording || operation == 0 || !self.is_operation_active(operation) {
            return Status::inactive(operation);
        }
        #[cfg(feature = "speech-vad")]
        if let Some(status) = self.capture_segments.status(operation) {
            return status;
        }
        Status::inactive(operation)
    }
}
