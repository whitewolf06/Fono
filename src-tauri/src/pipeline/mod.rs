//! Оркестратор голосового конвейера.
//!
//! FSM: `Idle → Listening → Transcribing → Processing → Injecting → Idle`.
//! См. `docs/architecture.md` → "Основной конвейер".
//!
//! Классическая диктовка запускается горячей клавишей в режиме hold или toggle.
//! Координатор связывает захват, VAD, STT, обработку и вставку с одной операцией;
//! запоздалые события не должны менять состояние следующей сессии.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::audio::{AudioRecorder, AudioRecordingOwner, RecordingWriter};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::stt::SttEngine;
use crate::types::PipelineState;
use fono_core::{
    OperationCancellation, OperationCoordinator, OperationEvent, OperationPhase, OperationResource,
    OperationSnapshot, OperationSource, TerminalReason,
};
#[cfg(test)]
mod capture_tests;
pub mod completion;
mod meter;
mod operation;
mod processing_choice;
mod publication;
mod recording;
pub(crate) use publication::{
    publish_terminal_claim, set_idle_if_no_operation, settle_outcome, sync_overlay_window,
};
pub use publication::{set_state, set_state_for_operation};
pub mod scheduler;
mod session_control;
use crate::audio::RecordingBuffer;

const RECORDING_SAMPLE_RATE: usize = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 5 * 60;
const MAX_RECORDING_SAMPLES: usize = RECORDING_SAMPLE_RATE * MAX_RECORDING_SECONDS;
const INITIAL_RECORDING_CAPACITY: usize = RECORDING_SAMPLE_RATE * 30;

/// Разделяемое состояние конвейера.
pub struct Pipeline {
    recording: Mutex<bool>,
    capture_operation: AtomicU64,
    stopped_capture_drops: Mutex<Option<(u64, u64)>>,
    /// Arc-буфер накопленных сэмплов, разделяемый с аудио-callback'ом cpal.
    writer: Mutex<Option<RecordingWriter>>,
    /// Adapter for the physical CPAL subscription; production keeps stream
    /// ownership inside AudioRecordingOwner while tests can inject failures.
    audio_owner: Box<dyn AudioRecorder<RecordingWriter, AppError>>,
    /// Set when the audio callback has filled the bounded recording buffer.
    recording_limit_reached: Arc<AtomicBool>,
    audio_level_bits: Arc<AtomicU32>,
    /// STT движок (переиспользуем между вызовами).
    stt: Arc<SttEngine>,
    /// Единственный владелец пользовательской операции и её lifecycle.
    operations: OperationCoordinator,
    service_operations: OperationCoordinator,
    pub completion: completion::CompletionGate,
    scheduler: Arc<scheduler::SttScheduler>,
    session_settings: Mutex<Option<(u64, crate::types::Settings)>>,
    processing_frozen_for: AtomicU64,
    meter_operation: AtomicU64,
}

impl Pipeline {
    pub fn new() -> Self {
        Self::new_with_audio_hub(fono_audio::AudioHub::new())
    }

    pub fn new_with_audio_hub(audio_hub: fono_audio::AudioHub) -> Self {
        Self::new_with_audio_owner(Box::new(AudioRecordingOwner::new(audio_hub)))
    }

    fn new_with_audio_owner(
        audio_owner: Box<dyn AudioRecorder<RecordingWriter, AppError>>,
    ) -> Self {
        Self {
            recording: Mutex::new(false),
            capture_operation: AtomicU64::new(0),
            stopped_capture_drops: Mutex::new(None),
            writer: Mutex::new(None),
            audio_owner,
            recording_limit_reached: Arc::new(AtomicBool::new(false)),
            audio_level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            stt: Arc::new(SttEngine::new()),
            operations: OperationCoordinator::new(),
            service_operations: OperationCoordinator::new(),
            completion: completion::CompletionGate::default(),
            scheduler: Arc::new(scheduler::SttScheduler::default()),
            session_settings: Mutex::new(None),
            processing_frozen_for: AtomicU64::new(0),
            meter_operation: AtomicU64::new(0),
        }
    }

    pub fn stt(&self) -> &Arc<SttEngine> {
        &self.stt
    }

    pub fn scheduler(&self) -> Arc<scheduler::SttScheduler> {
        self.scheduler.clone()
    }
    pub fn set_session_settings(&self, settings: crate::types::Settings) {
        self.set_session_settings_for(self.operation_id(), settings);
    }
    pub fn set_session_settings_for(
        &self,
        operation: u64,
        settings: crate::types::Settings,
    ) -> bool {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return false;
        }
        *self.session_settings.lock() = Some((operation, settings));
        true
    }
    pub fn session_settings(&self) -> Option<crate::types::Settings> {
        self.session_settings_for(self.operation_id())
    }
    pub fn session_settings_for(&self, operation: u64) -> Option<crate::types::Settings> {
        self.session_settings
            .lock()
            .as_ref()
            .filter(|(id, _)| *id == operation)
            .map(|(_, settings)| settings.clone())
    }
}

#[cfg(test)]
fn append_bounded(samples: &mut Vec<i16>, chunk: &[i16], maximum: usize) -> bool {
    let available = maximum.saturating_sub(samples.len());
    let accepted = available.min(chunk.len());
    samples.extend_from_slice(&chunk[..accepted]);
    accepted < chunk.len()
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
