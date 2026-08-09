//! Оркестратор голосового конвейера.
//!
//! FSM: `Idle → Listening → Transcribing → Processing → Injecting → Idle`.
//! См. `docs/architecture.md` → "Основной конвейер".
//!
//! На Этапе 0/1 реализован только базовый запуск: захват аудио в буфер
//! и возможность вручную триггернуть транскрипцию.
//! Push-to-talk и VAD добавляются на Этапе 3, wake word — на Этапе 4.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::{AudioRecordingOwner, RecordingWriter};
use crate::error::{AppError, AppResult};
use crate::injection;
use crate::llm::LlmClient;
use crate::operation::{
    OperationCoordinator, OperationEvent, OperationPhase, OperationSnapshot, OperationSource,
    TerminalReason,
};
use crate::state::AppState;
use crate::stt::SttEngine;
use crate::types::{AiMode, PipelineState};

const RECORDING_SAMPLE_RATE: usize = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 5 * 60;
const MAX_RECORDING_SAMPLES: usize = RECORDING_SAMPLE_RATE * MAX_RECORDING_SECONDS;
const INITIAL_RECORDING_CAPACITY: usize = RECORDING_SAMPLE_RATE * 30;

/// Разделяемое состояние конвейера.
pub struct Pipeline {
    recording: Mutex<bool>,
    /// Arc-буфер накопленных сэмплов, разделяемый с аудио-callback'ом cpal.
    writer: Mutex<Option<RecordingWriter>>,
    /// Единственный owner CPAL stream. Сам stream никогда не покидает свой поток.
    audio_owner: AudioRecordingOwner,
    /// Set when the audio callback has filled the bounded recording buffer.
    recording_limit_reached: Arc<AtomicBool>,
    /// STT движок (переиспользуем между вызовами).
    stt: Arc<SttEngine>,
    /// Единственный владелец пользовательской операции и её lifecycle.
    operations: OperationCoordinator,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            recording: Mutex::new(false),
            writer: Mutex::new(None),
            audio_owner: AudioRecordingOwner::new(),
            recording_limit_reached: Arc::new(AtomicBool::new(false)),
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
        match state {
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
        }
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
        let writer_lock = self.writer.lock();
        let Some(writer) = writer_lock.as_ref() else {
            return 0.0;
        };
        let buf = writer.lock();
        let take = buf.len().min(1_600); // 100 мс @ 16 кГц
        if take == 0 {
            return 0.0;
        }
        let window = &buf[buf.len() - take..];
        let sum_sq: i64 = window.iter().map(|&s| (s as i64) * (s as i64)).sum();
        ((sum_sq as f32 / take as f32).sqrt()) / i16::MAX as f32
    }

    /// Запускает запись аудио в накопительный буфер.
    pub fn start_recording(&self, device_id: Option<&str>) -> AppResult<()> {
        self.start_recording_from(device_id, OperationSource::Ui)
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
    pub fn start_recording_with_pre_roll(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
    ) -> AppResult<()> {
        self.start_recording_with_pre_roll_from(device_id, pre_roll, OperationSource::Ui)
    }

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
        drop(recording);

        stopped?;

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

/// Полный цикл диктовки: STT → (опционально LLM) → injection.
///
/// Предполагается, что модель STT уже загружена и settings корректны.
pub async fn run_full_pipeline(
    handle: &AppHandle,
    state: &AppState,
    pipeline: &Pipeline,
    samples: Vec<i16>,
) -> AppResult<()> {
    set_state(handle, state, PipelineState::Transcribing);

    let settings = state.settings();
    let language = settings.language.clone();

    let transcript = match pipeline.stt().transcribe(&samples, &language) {
        Ok(t) => t,
        Err(e) => {
            let _ = handle.emit("error", e.to_string());
            set_state(handle, state, PipelineState::Idle);
            return Err(e);
        }
    };

    tracing::info!(
        "transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    let final_text = match settings.ai_mode {
        AiMode::Off => transcript.text,
        mode => {
            set_state(handle, state, PipelineState::Processing);
            let client = LlmClient::from_settings(&settings);
            match client
                .process(&transcript.text, mode, settings.clean_prompt.as_deref())
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("LLM failed ({e}), returning raw transcript");
                    let _ = handle.emit("error", format!("LLM: {e}"));
                    transcript.text
                }
            }
        }
    };

    set_state(handle, state, PipelineState::Injecting);
    if let Err(e) = injection::inject_text(&final_text, settings.injection_mode) {
        let _ = handle.emit("error", e.to_string());
        set_state(handle, state, PipelineState::Idle);
        return Err(e);
    }

    set_state(handle, state, PipelineState::Idle);
    Ok(())
}

/// Обновляет состояние FSM, эмитит событие во фронтенд и управляет overlay-окном.
pub fn set_state(handle: &AppHandle, state: &AppState, new: PipelineState) {
    if let Some(event) = handle.state::<Pipeline>().sync_operation_state(new) {
        let _ = handle.emit("operation-state", event);
    }
    state.set_pipeline_state(new);
    tracing::debug!("pipeline state -> {new:?}");
    let _ = handle.emit("pipeline-state", new);

    // Показываем overlay только в активных состояниях; в Idle — прячем.
    if let Some(overlay) = handle.get_webview_window("overlay") {
        let _ = if matches!(new, PipelineState::Idle) {
            overlay.hide()
        } else {
            overlay.show()
        };
    }
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
    let _ = handle.emit("operation-state", event);
    state.set_pipeline_state(new);
    tracing::debug!(operation = operation_id, "pipeline state -> {new:?}");
    let _ = handle.emit("pipeline-state", new);
    if let Some(overlay) = handle.get_webview_window("overlay") {
        let _ = if matches!(new, PipelineState::Idle) {
            overlay.hide()
        } else {
            overlay.show()
        };
    }
    true
}

/// Точка входа фоновой задачи (wake word, idle-логика). На Этапе 0 — пусто.
pub async fn start_background(handle: AppHandle) -> AppResult<()> {
    tracing::info!("pipeline background task started");

    // Простой цикл-keeper: просыпается раз в секунду, оставляет место для
    // wake-word и idle-логики будущих этапов.
    let mut ticks = 0u64;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        ticks += 1;
        if ticks % 30 == 0 {
            tracing::trace!(
                "pipeline tick {} (state: {:?})",
                ticks,
                handle.state::<AppState>().pipeline_state()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::{OperationEvent, OperationPhase, OperationSource, TerminalReason};
    use crate::types::PipelineState;

    use super::{append_bounded, Pipeline};

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
