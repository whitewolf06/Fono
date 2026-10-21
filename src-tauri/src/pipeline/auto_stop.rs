//! Sample-clock silence detection. No RMS, STT, PCM trimming or wall-clock timeout.
use super::Pipeline;
use crate::{
    application::speech_gate::SpeechDecision,
    audio::RecordingWriter,
    types::{DictationMode, HotkeyMode, Settings},
};
use fono_core::OperationSource;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Inactive,
    Listening,
    Silence,
    Stopping,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Status {
    pub operation_id: u64,
    pub phase: Phase,
    pub timeout_ms: u64,
    pub remaining_ms: u64,
}

impl Status {
    pub fn inactive(operation_id: u64) -> Self {
        Self {
            operation_id,
            phase: Phase::Inactive,
            timeout_ms: 0,
            remaining_ms: 0,
        }
    }
    pub fn new(operation_id: u64, timeout_ms: u64) -> Self {
        Self {
            operation_id,
            phase: Phase::Listening,
            timeout_ms,
            remaining_ms: timeout_ms,
        }
    }
}

pub(crate) fn timeout(settings: &Settings, source: OperationSource, command: bool) -> Option<u64> {
    (!command
        && matches!(source, OperationSource::Ui | OperationSource::Hotkey)
        && settings.hotkey_mode == HotkeyMode::Toggle
        && settings.dictation_mode == DictationMode::Standard
        && (2_000..=10_000).contains(&settings.toggle_auto_stop_ms))
    .then_some(settings.toggle_auto_stop_ms)
}

pub(crate) struct SilenceClock {
    status: Status,
}

impl SilenceClock {
    pub fn new(operation: u64, timeout_ms: u64) -> Self {
        Self {
            status: Status::new(operation, timeout_ms),
        }
    }
    pub fn observe(&mut self, decision: &SpeechDecision) -> Status {
        let silent_samples = if decision.speaking {
            0
        } else {
            decision.processed.saturating_sub(decision.last_speech)
        };
        self.status.remaining_ms = self.status.timeout_ms.saturating_sub(silent_samples / 16);
        self.status.phase = if self.status.remaining_ms == 0 {
            Phase::Stopping
        } else if decision.speaking {
            Phase::Listening
        } else {
            Phase::Silence
        };
        self.status
    }
}

impl Pipeline {
    /// Capture identity and the bounded writer are resolved before spawning.
    pub(crate) fn auto_stop_reader_for(&self, operation: u64) -> Option<RecordingWriter> {
        let recording = self.recording.lock();
        if !*recording
            || self
                .capture_operation
                .load(std::sync::atomic::Ordering::Acquire)
                != operation
            || !self.is_operation_active(operation)
        {
            return None;
        }
        self.writer.lock().clone()
    }
    pub(crate) fn update_auto_stop(&self, value: Status) -> bool {
        self.while_editable_recording(value.operation_id, || {
            *self.auto_stop.lock() = value;
        })
    }
    pub fn auto_stop_status_for(&self, operation: u64) -> Status {
        let recording = self.recording.lock();
        if !*recording || operation == 0 || !self.is_operation_active(operation) {
            return Status::inactive(operation);
        }
        let status = *self.auto_stop.lock();
        if status.operation_id == operation {
            status
        } else {
            Status::inactive(operation)
        }
    }
}

#[cfg(test)]
mod tests;
