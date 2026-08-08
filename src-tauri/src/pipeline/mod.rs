//! Оркестратор голосового конвейера.
//!
//! FSM: `Idle → Listening → Transcribing → Processing → Injecting → Idle`.
//! См. `docs/architecture.md` → "Основной конвейер".
//!
//! На Этапе 0/1 реализован только базовый запуск: захват аудио в буфер
//! и возможность вручную триггернуть транскрипцию.
//! Push-to-talk и VAD добавляются на Этапе 3, wake word — на Этапе 4.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::{AudioCapture, AudioInputStream};
use crate::error::{AppError, AppResult};
use crate::injection;
use crate::llm::LlmClient;
use crate::state::AppState;
use crate::stt::SttEngine;
use crate::types::{AiMode, PipelineState};

const RECORDING_SAMPLE_RATE: usize = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 5 * 60;
const MAX_RECORDING_SAMPLES: usize = RECORDING_SAMPLE_RATE * MAX_RECORDING_SECONDS;
const INITIAL_RECORDING_CAPACITY: usize = RECORDING_SAMPLE_RATE * 30;

/// Обёртка над общим аудиопотоком, делающая её `Send + Sync`.
///
/// Внутренний cpal::Stream на Windows содержит `JoinHandle` и Win32 HANDLE,
/// которые по умолчанию не `Send`. Мы гарантируем, что stream
/// используется только из одного потока (через Mutex), поэтому
/// расширяем границы безопасности здесь.
struct StreamHolder(#[allow(dead_code)] AudioInputStream);
unsafe impl Send for StreamHolder {}
unsafe impl Sync for StreamHolder {}

/// Разделяемое состояние конвейера.
pub struct Pipeline {
    recording: Mutex<bool>,
    /// Arc-буфер накопленных сэмплов, разделяемый с аудио-callback'ом cpal.
    writer: Mutex<Option<Arc<Mutex<Vec<i16>>>>>,
    /// Активный аудио-поток. Dropstream останавливает захват.
    stream: Mutex<Option<StreamHolder>>,
    /// Set when the audio callback has filled the bounded recording buffer.
    recording_limit_reached: Arc<AtomicBool>,
    /// STT движок (переиспользуем между вызовами).
    stt: Arc<SttEngine>,
    /// Флаг отмены текущей диктовки (кнопка Stop в оверлее).
    cancelled: Arc<AtomicBool>,
    /// Monotonic id of the active dictation. It makes cancellation effective even
    /// when Whisper or an LLM request cannot be interrupted immediately.
    operation_id: AtomicU64,
    /// Флаг подтверждения текущей диктовки (кнопка ✓ в оверлее).
    confirmed: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            recording: Mutex::new(false),
            writer: Mutex::new(None),
            stream: Mutex::new(None),
            recording_limit_reached: Arc::new(AtomicBool::new(false)),
            stt: Arc::new(SttEngine::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
            operation_id: AtomicU64::new(0),
            confirmed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Запросить отмену текущей диктовки.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.operation_id.fetch_add(1, Ordering::SeqCst);
        tracing::debug!("pipeline: cancellation requested");
    }

    /// Starts a new user-visible operation and invalidates every older result.
    pub fn begin_operation(&self) -> u64 {
        self.cancelled.store(false, Ordering::SeqCst);
        self.confirmed.store(false, Ordering::SeqCst);
        self.operation_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn operation_id(&self) -> u64 {
        self.operation_id.load(Ordering::SeqCst)
    }

    pub fn is_operation_active(&self, operation_id: u64) -> bool {
        self.operation_id() == operation_id && !self.is_cancelled()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Подтвердить текущую диктовку (закончить запись досрочно).
    pub fn confirm(&self) {
        self.confirmed.store(true, Ordering::SeqCst);
        tracing::debug!("pipeline: confirmation requested");
    }

    pub fn reset_confirm(&self) {
        self.confirmed.store(false, Ordering::SeqCst);
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed.load(Ordering::SeqCst)
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
        self.start_recording_with_pre_roll(device_id, &[])
    }

    /// Запускает запись и добавляет короткий фрагмент до старта захвата.
    /// Нужен wake word: detector распознаёт фразу с задержкой, а pre-roll
    /// сохраняет слова, которые пользователь произнёс сразу после неё.
    pub fn start_recording_with_pre_roll(
        &self,
        device_id: Option<&str>,
        pre_roll: &[i16],
    ) -> AppResult<()> {
        let mut recording = self.recording.lock();
        if *recording {
            tracing::warn!("start_recording called while already recording");
            return Err(AppError::Busy("audio recording is already active".into()));
        }
        if pre_roll.len() > MAX_RECORDING_SAMPLES {
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
        let writer_for_callback = Arc::clone(&writer);
        let limit_reached = Arc::clone(&self.recording_limit_reached);

        let stream = AudioCapture::start(device_id, move |chunk: &[i16]| {
            let mut samples = writer_for_callback.lock();
            if append_bounded(&mut samples, chunk, MAX_RECORDING_SAMPLES) {
                limit_reached.store(true, Ordering::Relaxed);
            }
        })?;

        self.begin_operation();
        *self.stream.lock() = Some(StreamHolder(stream));
        *self.writer.lock() = Some(writer);
        *recording = true;
        tracing::info!(
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

        // Keep the lifecycle lock until the old stream is fully stopped. A
        // concurrent start cannot install a new stream that this call would
        // accidentally take and drop.
        if let Some(holder) = self.stream.lock().take() {
            drop(holder);
        }
        let writer = self.writer.lock().take();
        *recording = false;
        drop(recording);

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
    use super::{append_bounded, Pipeline};

    #[test]
    fn cancellation_invalidates_in_flight_operation() {
        let pipeline = Pipeline::new();
        let operation = pipeline.begin_operation();
        assert!(pipeline.is_operation_active(operation));

        pipeline.cancel();
        assert!(!pipeline.is_operation_active(operation));

        let next_operation = pipeline.begin_operation();
        assert!(pipeline.is_operation_active(next_operation));
        assert_ne!(operation, next_operation);
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
