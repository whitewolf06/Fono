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
mod processing_choice;
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

    /// Запросить отмену текущей диктовки.
    pub fn cancel(&self) -> Option<OperationEvent> {
        let operation = self.operations.current()?;
        self.cancel_for(operation.id)
    }

    /// Delayed callbacks can cancel only the operation they captured earlier.
    pub fn cancel_for(&self, operation: u64) -> Option<OperationEvent> {
        let _capture = self.recording.lock();
        let event = self.operations.cancel(operation);
        if event.is_some() {
            self.scheduler.release_reservation(operation);
            tracing::debug!(operation, "pipeline: cancellation requested");
        }
        event
    }

    pub fn shutdown(&self) -> Option<OperationEvent> {
        let event = self.cancel();
        let _ = self.stop_recording();
        self.audio_owner.shutdown();
        event
    }

    pub fn finish_operation(
        &self,
        operation_id: u64,
        reason: TerminalReason,
    ) -> Option<OperationEvent> {
        self.scheduler.release_reservation(operation_id);
        self.operations.finish(operation_id, reason)
    }

    pub fn operation_id(&self) -> u64 {
        self.operations
            .current()
            .map_or(0, |operation| operation.id)
    }

    pub fn current_operation(&self) -> Option<OperationSnapshot> {
        self.operations.current()
    }
    pub fn has_active_service_operation(&self) -> bool {
        self.service_operations.current().is_some()
    }

    pub fn is_operation_active(&self, operation_id: u64) -> bool {
        self.operations.is_active(operation_id)
    }

    /// Commit an in-process side effect before cancellation or replacement.
    /// The closure must not reacquire this lock, run inference or send native input.
    pub(crate) fn while_operation<T>(
        &self,
        operation: u64,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return None;
        }
        Some(action())
    }

    /// Idle cleanup and wake resumption complete before a new capture can attach.
    pub(crate) fn while_idle(&self, action: impl FnOnce()) -> bool {
        let _capture = self.recording.lock();
        if self.current_operation().is_some() {
            return false;
        }
        action();
        true
    }

    pub fn cancellation(&self, operation_id: u64) -> Option<OperationCancellation> {
        self.operations.cancellation(operation_id)
    }

    /// Service lifecycle is independent from interactive dictation. The STT
    /// scheduler pauses a service at a bounded window when dictation starts.
    pub fn start_service_transcription(&self) -> AppResult<(u64, OperationCancellation)> {
        let _update_admission = crate::application::updates::activity::begin()?;
        let operation = self.service_operations.start(OperationSource::Service)?;
        if let Err(error) = self
            .service_operations
            .acquire_resource(operation.id, OperationResource::Stt)
        {
            let _ = self
                .service_operations
                .finish(operation.id, TerminalReason::Failed);
            return Err(error.into());
        }
        let _ = self
            .service_operations
            .transition(operation.id, OperationPhase::Transcribing);
        let cancellation = self
            .service_operations
            .cancellation(operation.id)
            .expect("new service operation must own cancellation");
        Ok((operation.id, cancellation))
    }

    /// Подтвердить текущую диктовку (закончить запись досрочно).
    pub fn confirm(&self) {
        if let Some(operation) = self.operations.current() {
            self.confirm_for(operation.id);
        }
    }

    pub fn is_operation_confirmed(&self, operation_id: u64) -> bool {
        self.operations
            .current()
            .is_some_and(|operation| operation.id == operation_id && operation.confirmed)
    }

    pub fn sync_operation_state(&self, state: PipelineState) -> Option<OperationEvent> {
        let operation = self.operations.current()?;
        self.sync_operation_state_for(operation.id, state, TerminalReason::Completed)
    }

    pub fn sync_operation_state_for(
        &self,
        operation_id: u64,
        state: PipelineState,
        terminal_reason: TerminalReason,
    ) -> Option<OperationEvent> {
        let event = match state {
            PipelineState::Idle => self.operations.finish(operation_id, terminal_reason),
            PipelineState::Error => self.operations.finish(operation_id, TerminalReason::Failed),
            PipelineState::Listening => self
                .operations
                .transition(operation_id, OperationPhase::Recording),
            PipelineState::Transcribing => self
                .operations
                .transition(operation_id, OperationPhase::Transcribing),
            PipelineState::Processing => self
                .operations
                .transition(operation_id, OperationPhase::Processing),
            PipelineState::AwaitingAction => self
                .operations
                .transition(operation_id, OperationPhase::AwaitingAction),
            PipelineState::Injecting => self
                .operations
                .transition(operation_id, OperationPhase::Injecting),
        };

        if event.is_some() {
            if matches!(state, PipelineState::Idle | PipelineState::Error) {
                self.scheduler.release_reservation(operation_id);
            }
            match state {
                PipelineState::Transcribing => {
                    self.operations
                        .release_resource(operation_id, OperationResource::Audio);
                    let _ = self
                        .operations
                        .acquire_resource(operation_id, OperationResource::Stt);
                }
                PipelineState::Processing | PipelineState::AwaitingAction => {
                    self.operations
                        .release_resource(operation_id, OperationResource::Stt);
                    self.operations
                        .release_resource(operation_id, OperationResource::Injection);
                    self.scheduler.release_reservation(operation_id);
                }
                PipelineState::Injecting => {
                    self.operations
                        .release_resource(operation_id, OperationResource::Stt);
                    let _ = self
                        .operations
                        .acquire_resource(operation_id, OperationResource::Injection);
                }
                PipelineState::Idle | PipelineState::Error | PipelineState::Listening => {}
            }
        }
        event
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

    pub fn recording_window(&self, from: u64, maximum: usize) -> (u64, Vec<i16>) {
        self.writer
            .lock()
            .as_ref()
            .map(|w| w.lock().window(from, maximum))
            .unwrap_or((from, Vec::new()))
    }
    pub fn recording_end(&self) -> u64 {
        self.writer.lock().as_ref().map_or(0, |w| w.lock().end())
    }
    pub fn recording_start(&self) -> u64 {
        self.writer.lock().as_ref().map_or(0, |w| w.lock().start())
    }
    pub fn finish_service_operation(&self, operation: u64, reason: TerminalReason) {
        let _ = self.service_operations.finish(operation, reason);
    }
    pub fn discard_audio_before(&self, offset: u64) {
        if let Some(writer) = self.writer.lock().as_ref() {
            writer.lock().discard_before(offset);
        }
    }

    pub fn is_recording(&self) -> bool {
        *self.recording.lock()
    }

    pub fn recording_limit_reached(&self) -> bool {
        self.recording_limit_reached.load(Ordering::SeqCst)
    }

    /// Нормированный RMS уровень звука последних ~100 мс записи (0.0..1.0).
    /// Уровень для визуального индикатора; решения о речи принимает neural VAD.
    /// Возвращает 0.0, если запись не идёт или буфер пока пуст.
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.audio_level_bits.load(Ordering::Relaxed))
    }

    pub fn start_recording_from(
        &self,
        device_id: Option<&str>,
        source: OperationSource,
    ) -> AppResult<u64> {
        self.start_recording_with_pre_roll_from(device_id, &[], source)
    }

    /// Запускает запись и добавляет короткий фрагмент до старта захвата.
    /// Нужен wake word: detector распознаёт фразу с задержкой, а pre-roll
    /// сохраняет слова, которые пользователь произнёс сразу после неё.
    pub fn start_recording_with_pre_roll_from(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
        source: OperationSource,
    ) -> AppResult<u64> {
        self.start_capture(device_id, pre_roll, source, None, false)
    }

    pub fn start_capture(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
        source: OperationSource,
        cursor: Option<(u64, u64)>,
        live: bool,
    ) -> AppResult<u64> {
        let _update_admission = crate::application::updates::activity::begin()?;
        let mut recording = self.recording.lock();
        if *recording {
            tracing::warn!("start_recording called while already recording");
            return Err(AppError::Busy("audio recording is already active".into()));
        }
        let operation = self.operations.start(source)?;
        self.operations
            .acquire_resource(operation.id, OperationResource::Audio)?;
        if pre_roll.len() > MAX_RECORDING_SAMPLES {
            let _ = self.operations.finish(operation.id, TerminalReason::Failed);
            return Err(AppError::Audio(format!(
                "pre-roll exceeds the maximum recording length of {MAX_RECORDING_SECONDS} seconds"
            )));
        }

        self.recording_limit_reached.store(false, Ordering::SeqCst);
        let maximum = if live {
            RECORDING_SAMPLE_RATE * 120
        } else {
            MAX_RECORDING_SAMPLES
        };
        let writer = Arc::new(Mutex::new(RecordingBuffer::new(
            pre_roll,
            INITIAL_RECORDING_CAPACITY,
        )));
        self.scheduler.reserve(operation.id);
        if let Err(error) = self.audio_owner.start_after(
            device_id,
            Arc::clone(&writer),
            Arc::clone(&self.recording_limit_reached),
            Arc::clone(&self.audio_level_bits),
            maximum,
            cursor,
        ) {
            self.scheduler.release_reservation(operation.id);
            let _ = self.operations.finish(operation.id, TerminalReason::Failed);
            return Err(error);
        }

        *self.writer.lock() = Some(writer);
        *self.stopped_capture_drops.lock() = None;
        self.capture_operation
            .store(operation.id, Ordering::Release);
        *recording = true;
        tracing::info!(
            operation = operation.id,
            source = ?source,
            "recording started, bounded writer attached (pre_roll={:.2}s, max={}s)",
            pre_roll.len() as f32 / RECORDING_SAMPLE_RATE as f32,
            MAX_RECORDING_SECONDS
        );
        Ok(operation.id)
    }

    /// Collect only the writer owned by this operation, under the capture lock.
    pub fn stop_recording_for(&self, operation: u64) -> AppResult<Option<Vec<i16>>> {
        let mut recording = self.recording.lock();
        if operation == 0 || self.capture_operation.load(Ordering::Acquire) != operation {
            return Ok(None);
        }
        self.stop_capture_locked(&mut recording)?;
        let writer = self.writer.lock().take();
        self.capture_operation.store(0, Ordering::Release);
        Ok(writer.map(|writer| writer.lock().take()))
    }

    pub fn stop_recording(&self) -> AppResult<Vec<i16>> {
        let operation = self.capture_operation.load(Ordering::Acquire);
        Ok(self.stop_recording_for(operation)?.unwrap_or_default())
    }

    /// Final capture loss belongs to exactly one still-current operation. A late
    /// collector must never read the replacement recording's counter or old loss.
    pub(crate) fn capture_dropped_packets_for(&self, operation: u64) -> Option<u64> {
        let _capture = self.recording.lock();
        if operation == 0 || self.operation_id() != operation {
            return None;
        }
        self.stopped_capture_drops
            .lock()
            .as_ref()
            .filter(|(owner, _)| *owner == operation)
            .map(|(_, count)| *count)
    }

    /// Physical capture stops immediately; its queued packets drain before the
    /// lock is released. The live decoder retains its writer for final windows.
    pub fn stop_capture_for(&self, operation: u64) -> AppResult<bool> {
        let mut recording = self.recording.lock();
        if operation == 0 || self.capture_operation.load(Ordering::Acquire) != operation {
            return Ok(false);
        }
        self.stop_capture_locked(&mut recording)?;
        Ok(true)
    }

    pub fn stop_capture(&self) -> AppResult<()> {
        self.stop_capture_for(self.capture_operation.load(Ordering::Acquire))
            .map(|_| ())
    }

    fn stop_capture_locked(&self, recording: &mut bool) -> AppResult<()> {
        if !*recording {
            return Ok(());
        }
        let stopped = self.audio_owner.stop();
        *self.stopped_capture_drops.lock() = self
            .audio_owner
            .dropped_capture_packets()
            .map(|count| (self.capture_operation.load(Ordering::Acquire), count));
        *recording = false;
        self.audio_level_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        self.operations.release_resource(
            self.capture_operation.load(Ordering::Acquire),
            OperationResource::Audio,
        );
        stopped?;
        if self.recording_limit_reached() {
            tracing::warn!(
                "recording buffer reached its limit; input stopped with an explicit warning"
            );
        }
        Ok(())
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

/// Обновляет состояние FSM, эмитит событие во фронтенд и управляет overlay-окном.
pub fn set_state(handle: &AppHandle, state: &AppState, new: PipelineState) {
    if let Some(event) = handle.state::<Pipeline>().sync_operation_state(new) {
        crate::events::emit_operation(handle, event);
    }
    state.set_pipeline_state(new);
    tracing::debug!("pipeline state -> {new:?}");
    crate::events::emit_pipeline_state(handle, new);
    meter::sync(handle, new);

    sync_overlay_window(handle, new);
}

/// Applies a state transition only if it belongs to the specified operation.
/// This prevents an old async STT/LLM result from hiding the overlay or
/// finishing a newer dictation after cancellation.
pub fn set_state_for_operation(
    handle: &AppHandle,
    state: &AppState,
    pipeline: &Pipeline,
    operation_id: u64,
    new: PipelineState,
    terminal_reason: TerminalReason,
) -> bool {
    let _capture = pipeline.recording.lock();
    let Some(event) = pipeline.sync_operation_state_for(operation_id, new, terminal_reason) else {
        tracing::debug!(
            operation = operation_id,
            ?new,
            "ignoring stale pipeline state update"
        );
        return false;
    };
    crate::events::emit_operation(handle, event);
    state.set_pipeline_state(new);
    tracing::debug!(operation = operation_id, "pipeline state -> {new:?}");
    crate::events::emit_pipeline_state(handle, new);
    meter::sync(handle, new);
    // Publish under the capture fence, but never hold it across window dispatch.
    drop(_capture);
    sync_overlay_window(handle, new);
    true
}

/// Cancellation already emitted its terminal event; publish Idle only while vacant.
pub(crate) fn set_idle_if_no_operation(
    handle: &AppHandle,
    state: &AppState,
    pipeline: &Pipeline,
) -> bool {
    let committed = pipeline.while_idle(|| {
        state.set_pipeline_state(PipelineState::Idle);
        crate::events::emit_pipeline_state(handle, PipelineState::Idle);
        meter::sync(handle, PipelineState::Idle);
    });
    if committed {
        sync_overlay_window(handle, PipelineState::Idle);
    }
    committed
}

pub(crate) fn sync_overlay_window(handle: &AppHandle, _state: PipelineState) {
    // Visibility is recomputed when the UI task executes. A queued old Idle
    // notification must not hide the overlay of a newer recording.
    crate::overlay::request_visibility(handle);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use std::sync::Arc;

    use crate::audio::{AudioRecorder, RecordingWriter};
    use crate::error::{AppError, AppResult};
    use crate::types::PipelineState;
    use fono_core::{
        OperationEvent, OperationPhase, OperationResource, OperationSource, TerminalReason,
    };

    use super::{append_bounded, Pipeline};

    struct FailingAudioRecorder;

    impl AudioRecorder<RecordingWriter, AppError> for FailingAudioRecorder {
        fn start(
            &self,
            _device_id: Option<&str>,
            _writer: RecordingWriter,
            _limit_reached: Arc<AtomicBool>,
            _level_bits: Arc<AtomicU32>,
            _maximum_samples: usize,
        ) -> AppResult<()> {
            Err(AppError::Audio("injected audio acquisition failure".into()))
        }

        fn stop(&self) -> AppResult<()> {
            Ok(())
        }

        fn shutdown(&self) {}
    }

    #[test]
    fn cancellation_invalidates_in_flight_operation() {
        let pipeline = Pipeline::new();
        let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
        assert!(pipeline.is_operation_active(operation.id));

        pipeline.cancel();
        assert!(!pipeline.is_operation_active(operation.id));

        let next_operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
        assert!(pipeline.is_operation_active(next_operation.id));
        assert_ne!(operation.id, next_operation.id);
    }

    #[test]
    fn audio_acquisition_failure_releases_operation_and_resource_lease() {
        let pipeline = Pipeline::new_with_audio_owner(Box::new(FailingAudioRecorder));

        let error = pipeline
            .start_recording_from(None, OperationSource::WakeWord)
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("injected audio acquisition failure"));
        assert_eq!(pipeline.operation_id(), 0);
        assert!(!pipeline.is_recording());
    }

    #[test]
    fn shutdown_is_idempotent_and_finishes_the_active_operation() {
        let pipeline = Pipeline::new();
        let operation = pipeline.operations.start(OperationSource::Ui).unwrap();

        assert!(matches!(
            pipeline.shutdown(),
            Some(OperationEvent::Finished(terminal))
                if terminal.id == operation.id && terminal.reason == TerminalReason::Cancelled
        ));
        assert!(pipeline.shutdown().is_none());
        assert!(!pipeline.is_operation_active(operation.id));
    }

    #[test]
    fn service_lifecycle_does_not_block_interactive_recording() {
        let pipeline = Pipeline::new();

        let (operation_id, cancellation) = pipeline.start_service_transcription().unwrap();

        assert_eq!(pipeline.operation_id(), 0);
        assert!(!cancellation.is_cancelled());
        assert_eq!(
            pipeline.service_operations.resources(operation_id),
            Some(vec![OperationResource::Stt])
        );
        let interactive = pipeline.operations.start(OperationSource::Ui).unwrap();
        pipeline.finish_service_operation(operation_id, TerminalReason::Completed);
        assert!(pipeline.is_operation_active(interactive.id));
        assert!(pipeline.service_operations.current().is_none());
    }

    #[test]
    fn pipeline_state_updates_are_bound_to_the_active_operation() {
        let pipeline = Pipeline::new();
        let operation = pipeline
            .operations
            .start(OperationSource::WakeWord)
            .unwrap();

        assert!(matches!(
            pipeline.sync_operation_state(PipelineState::Transcribing),
            Some(OperationEvent::PhaseChanged(snapshot))
                if snapshot.id == operation.id && snapshot.phase == OperationPhase::Transcribing
        ));
        assert!(pipeline
            .sync_operation_state(PipelineState::Processing)
            .is_some());
        assert!(matches!(
            pipeline.sync_operation_state(PipelineState::Idle),
            Some(OperationEvent::Finished(terminal))
                if terminal.id == operation.id && terminal.reason == TerminalReason::Completed
        ));
        assert!(!pipeline.is_operation_active(operation.id));
    }

    #[test]
    fn awaiting_action_releases_stt_but_keeps_operation_and_retries() {
        let pipeline = Pipeline::new();
        let operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::Transcribing,
                TerminalReason::Completed,
            )
            .unwrap();
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::AwaitingAction,
                TerminalReason::Completed,
            )
            .unwrap();
        assert!(pipeline.is_operation_active(operation.id));
        assert!(!pipeline
            .operations
            .resources(operation.id)
            .unwrap()
            .contains(&OperationResource::Stt));
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::Processing,
                TerminalReason::Completed,
            )
            .unwrap();
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::AwaitingAction,
                TerminalReason::Completed,
            )
            .unwrap();
        pipeline
            .sync_operation_state_for(operation.id, PipelineState::Idle, TerminalReason::Cancelled)
            .unwrap();
        assert!(!pipeline.is_operation_active(operation.id));
    }

    #[test]
    fn bounded_recording_never_exceeds_its_sample_limit() {
        let mut samples = vec![1, 2];

        assert!(append_bounded(&mut samples, &[3, 4, 5], 4));
        assert_eq!(samples, vec![1, 2, 3, 4]);
        assert!(append_bounded(&mut samples, &[6], 4));
        assert_eq!(samples.len(), 4);
    }

    #[test]
    fn bounded_recording_accepts_a_complete_chunk_when_capacity_is_available() {
        let mut samples = Vec::new();

        assert!(!append_bounded(&mut samples, &[1, 2, 3], 4));
        assert_eq!(samples, vec![1, 2, 3]);
    }
}
