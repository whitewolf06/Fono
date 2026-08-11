//! Оркестратор голосового конвейера.
//!
//! FSM: `Idle → Listening → Transcribing → Processing → Injecting → Idle`.
//! См. `docs/architecture.md` → "Основной конвейер".
//!
//! На Этапе 0/1 реализован только базовый запуск: захват аудио в буфер
//! и возможность вручную триггернуть транскрипцию.
//! Push-to-talk и VAD добавляются на Этапе 3, wake word — на Этапе 4.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::audio::{AudioRecorder, AudioRecordingOwner, RecordingWriter};
use crate::error::{AppError, AppResult};
use crate::operation::{
    OperationCancellation, OperationCoordinator, OperationEvent, OperationPhase, OperationResource,
    OperationSnapshot, OperationSource, TerminalReason,
};
use crate::state::AppState;
use crate::stt::SttEngine;
use crate::types::PipelineState;

const RECORDING_SAMPLE_RATE: usize = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 5 * 60;
const MAX_RECORDING_SAMPLES: usize = RECORDING_SAMPLE_RATE * MAX_RECORDING_SECONDS;
const INITIAL_RECORDING_CAPACITY: usize = RECORDING_SAMPLE_RATE * 30;

/// Разделяемое состояние конвейера.
pub struct Pipeline {
    recording: Mutex<bool>,
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
}

impl Pipeline {
    pub fn new() -> Self {
        Self::new_with_audio_hub(fono_wake::AudioHub::new())
    }

    pub fn new_with_audio_hub(audio_hub: fono_wake::AudioHub) -> Self {
        Self::new_with_audio_owner(Box::new(AudioRecordingOwner::new(audio_hub)))
    }

    fn new_with_audio_owner(
        audio_owner: Box<dyn AudioRecorder<RecordingWriter, AppError>>,
    ) -> Self {
        Self {
            recording: Mutex::new(false),
            writer: Mutex::new(None),
            audio_owner,
            recording_limit_reached: Arc::new(AtomicBool::new(false)),
            audio_level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            stt: Arc::new(SttEngine::new()),
            operations: OperationCoordinator::new(),
        }
    }

    /// Запросить отмену текущей диктовки.
    pub fn cancel(&self) -> Option<OperationEvent> {
        let operation = self.operations.current()?;
        let event = self.operations.cancel(operation.id);
        if event.is_some() {
            tracing::debug!(operation = operation.id, "pipeline: cancellation requested");
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

    pub fn is_operation_active(&self, operation_id: u64) -> bool {
        self.operations.is_active(operation_id)
    }

    pub fn cancellation(&self, operation_id: u64) -> Option<OperationCancellation> {
        self.operations.cancellation(operation_id)
    }

    /// Подтвердить текущую диктовку (закончить запись досрочно).
    pub fn confirm(&self) {
        if let Some(operation) = self.operations.current() {
            let _ = self.operations.confirm(operation.id);
            tracing::debug!(operation = operation.id, "pipeline: confirmation requested");
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
            PipelineState::Injecting => self
                .operations
                .transition(operation_id, OperationPhase::Injecting),
        };

        if event.is_some() {
            match state {
                PipelineState::Transcribing => {
                    self.operations
                        .release_resource(operation_id, OperationResource::Audio);
                    let _ = self
                        .operations
                        .acquire_resource(operation_id, OperationResource::Stt);
                }
                PipelineState::Processing => {
                    self.operations
                        .release_resource(operation_id, OperationResource::Stt);
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

    pub fn is_recording(&self) -> bool {
        *self.recording.lock()
    }

    pub fn recording_limit_reached(&self) -> bool {
        self.recording_limit_reached.load(Ordering::SeqCst)
    }

    /// Нормированный RMS уровень звука последних ~100 мс записи (0.0..1.0).
    /// Используется wake-диктовкой для определения тишины (стоп по VAD),
    /// когда основной writer-буфер уже пишется аудио-потоком.
    /// Возвращает 0.0, если запись не идёт или буфер пока пуст.
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.audio_level_bits.load(Ordering::Relaxed))
    }

    pub fn start_recording_from(
        &self,
        device_id: Option<&str>,
        source: OperationSource,
    ) -> AppResult<()> {
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
    ) -> AppResult<()> {
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
        let mut samples = Vec::with_capacity(
            INITIAL_RECORDING_CAPACITY
                .max(pre_roll.len())
                .min(MAX_RECORDING_SAMPLES),
        );
        samples.extend_from_slice(pre_roll);
        let writer = Arc::new(Mutex::new(samples));
        if let Err(error) = self.audio_owner.start(
            device_id,
            Arc::clone(&writer),
            Arc::clone(&self.recording_limit_reached),
            Arc::clone(&self.audio_level_bits),
            MAX_RECORDING_SAMPLES,
        ) {
            let _ = self.operations.finish(operation.id, TerminalReason::Failed);
            return Err(error);
        }

        *self.writer.lock() = Some(writer);
        *recording = true;
        tracing::info!(
            operation = operation.id,
            source = ?source,
            "recording started, bounded writer attached (pre_roll={:.2}s, max={}s)",
            pre_roll.len() as f32 / RECORDING_SAMPLE_RATE as f32,
            MAX_RECORDING_SECONDS
        );
        Ok(())
    }

    /// Останавливает запись и возвращает накопленные сэмплы.
    pub fn stop_recording(&self) -> AppResult<Vec<i16>> {
        let mut recording = self.recording.lock();
        if !*recording {
            tracing::warn!("stop_recording called but was not recording");
            return Ok(Vec::new());
        }

        // Keep the lifecycle lock until the owner confirms that the old stream
        // was dropped. A concurrent start cannot install a new recording while
        // this call is releasing the previous one.
        let stopped = self.audio_owner.stop();
        let writer = self.writer.lock().take();
        *recording = false;
        self.audio_level_bits
            .store(0.0f32.to_bits(), Ordering::Relaxed);
        drop(recording);

        stopped?;
        if let Some(operation) = self.operations.current() {
            self.operations
                .release_resource(operation.id, OperationResource::Audio);
        }

        let writer = writer.ok_or_else(|| AppError::Audio("запись не была запущена".into()))?;
        let samples = std::mem::take(&mut *writer.lock());
        if self.recording_limit_reached() {
            tracing::warn!(
                max_seconds = MAX_RECORDING_SECONDS,
                "recording buffer limit reached; additional audio was discarded"
            );
        }
        tracing::info!(
            "recording stopped, captured {} samples (~{:.2}s @ 16kHz)",
            samples.len(),
            samples.len() as f32 / RECORDING_SAMPLE_RATE as f32
        );
        Ok(samples)
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
    sync_overlay_window(handle, new);
    true
}

fn sync_overlay_window(handle: &AppHandle, state: PipelineState) {
    let Some(overlay) = handle.get_webview_window("overlay") else {
        return;
    };
    sync_overlay_visibility(state, |visible| {
        if visible {
            overlay.show()
        } else {
            overlay.hide()
        }
    });
}

/// Keeps window-management failures outside the operation lifecycle. The
/// caller has already committed its pipeline transition before this adapter is
/// invoked, so a missing or closing overlay cannot retain a resource lease.
fn sync_overlay_visibility<E>(state: PipelineState, set_visible: impl FnOnce(bool) -> Result<(), E>)
where
    E: std::fmt::Display,
{
    let visible = !matches!(state, PipelineState::Idle);
    if let Err(error) = set_visible(visible) {
        tracing::warn!(%error, visible, ?state, "could not synchronize overlay visibility");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use std::sync::Arc;

    use parking_lot::Mutex;

    use crate::audio::{AudioRecorder, RecordingWriter};
    use crate::error::{AppError, AppResult};
    use crate::operation::{OperationEvent, OperationPhase, OperationSource, TerminalReason};
    use crate::types::PipelineState;

    use super::{append_bounded, sync_overlay_visibility, Pipeline};

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
    fn overlay_fault_injection_cannot_undo_a_terminal_transition() {
        let pipeline = Pipeline::new();
        let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::Transcribing,
                TerminalReason::Completed,
            )
            .expect("start STT phase");
        pipeline
            .sync_operation_state_for(
                operation.id,
                PipelineState::Injecting,
                TerminalReason::Completed,
            )
            .expect("start injection phase");
        assert!(pipeline.is_operation_active(operation.id));

        let observed = Mutex::new(Vec::new());

        sync_overlay_visibility(PipelineState::Injecting, |visible| {
            observed.lock().push(visible);
            Err("injected overlay show failure")
        });
        sync_overlay_visibility(PipelineState::Idle, |visible| {
            observed.lock().push(visible);
            Err("injected overlay hide failure")
        });

        assert_eq!(*observed.lock(), vec![true, false]);

        pipeline
            .sync_operation_state_for(operation.id, PipelineState::Idle, TerminalReason::Completed)
            .expect("finish operation before hiding overlay");
        sync_overlay_visibility(PipelineState::Idle, |_| {
            Err("injected terminal overlay hide failure")
        });
        assert!(!pipeline.is_operation_active(operation.id));
        assert!(pipeline.operations.resources(operation.id).is_none());
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
