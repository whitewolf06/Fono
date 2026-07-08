//! Wake word детекция через whisper.cpp continuous.
//!
//! Архитектура:
//!   - Фоновый поток постоянно захватывает аудио чанками по ~1.5 сек.
//!   - Каждый чанк транскрибируется через whisper с `tiny` моделью (быстро).
//!   - Если транскрипт содержит фразу-триггер (напр. «эй ассистент»),
//!     поток сигнализирует основному pipeline начать запись диктовки.
//!   - VAD в чанке отсекает тишину — не гоняем whisper на пустом аудио.
//!
//! CPU в idle: ~5-10% (tiny модель на 1.5 сек аудио = ~0.2 сек обработки).
//! Можно еще оптимизировать: слушать только когда есть звук (VAD-gating).

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::stt::SttEngine;
use crate::vad;

/// События от wake word потока.
#[derive(Debug, Clone)]
pub enum WakeEvent {
    /// Обнаружена wake-фраза. Можно начинать запись диктовки.
    Detected { transcription: String },
    /// Ошибка в потоке wake word.
    Error(String),
    /// Состояние wake word (для UI).
    Status(WakeStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeStatus {
    /// Слушаем wake word (idle).
    Listening,
    /// Транскрибируем чанк (занято).
    Processing,
    /// Wake word обнаружена, ждём основной pipeline.
    Triggered,
    /// Приостановлено (push-to-talk активен или pipeline занят).
    Paused,
}

/// Конфигурация wake word детектора.
#[derive(Debug, Clone)]
pub struct WakeWordConfig {
    /// Фраза-триггер (lowercase, без пунктуации).
    pub phrase: String,
    /// Длительность чанка захвата в миллисекундах.
    pub chunk_ms: u64,
    /// Минимальный RMS уровень в чанке, чтобы гнать его через whisper.
    /// Если тише — пропускаем (экономим CPU).
    pub vad_threshold: f32,
    /// Загружать ли отдельную tiny-модель для wake word, или использовать основную.
    pub use_tiny_model: bool,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            phrase: "эй ассистент".to_string(),
            chunk_ms: 1500,
            vad_threshold: 0.015,
            use_tiny_model: true,
        }
    }
}

/// Wake word детектор. Запускает фоновый поток, который слушает микрофон
/// и транскрибирует чанки через whisper, ища фразу-триггер.
pub struct WakeWordDetector {
    config: Arc<Mutex<WakeWordConfig>>,
    status: Arc<Mutex<WakeStatus>>,
    /// Канал событий: wake word поток → pipeline.
    events_tx: Mutex<Option<Sender<WakeEvent>>>,
    /// Handle фонового потока (чтобы можно было остановить).
    thread_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Флаг остановки.
    stop_flag: Arc<Mutex<bool>>,
}

impl WakeWordDetector {
    pub fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(WakeWordConfig::default())),
            status: Arc::new(Mutex::new(WakeStatus::Paused)),
            events_tx: Mutex::new(None),
            thread_handle: Mutex::new(None),
            stop_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn status(&self) -> WakeStatus {
        *self.status.lock()
    }

    pub fn set_config(&self, config: WakeWordConfig) {
        *self.config.lock() = config;
    }

    pub fn set_phrase(&self, phrase: String) {
        self.config.lock().phrase = phrase.to_lowercase();
    }

    /// Запускает фоновый поток wake word.
    ///
    /// `stt` — STT движок (должен быть загружен с tiny моделью, если use_tiny_model=true).
    /// `audio_device_id` — ID микрофона.
    /// `on_event` — callback для обработки событий (старт диктовки и т.д.).
    pub fn start<F>(
        &self,
        stt: Arc<SttEngine>,
        audio_device_id: Option<String>,
        on_event: F,
    ) -> AppResult<()>
    where
        F: Fn(WakeEvent) + Send + 'static,
    {
        if self.thread_handle.lock().is_some() {
            tracing::warn!("wake word: already running");
            return Ok(());
        }

        *self.stop_flag.lock() = false;
        *self.status.lock() = WakeStatus::Listening;

        let (tx, rx): (Sender<WakeEvent>, Receiver<WakeEvent>) = unbounded();
        *self.events_tx.lock() = Some(tx);

        let config = self.config.lock().clone();
        let status = self.status.clone();
        let stop_flag = self.stop_flag.clone();

        let handle = std::thread::Builder::new()
            .name("wake-word".to_string())
            .spawn(move || {
                wake_word_loop(stt, audio_device_id, config, status, stop_flag, rx, on_event);
            })
            .map_err(|e| AppError::Internal(format!("wake word thread: {e}")))?;

        *self.thread_handle.lock() = Some(handle);
        tracing::info!("wake word: background thread started");
        Ok(())
    }

    /// Останавливает фоновый поток.
    pub fn stop(&self) {
        *self.stop_flag.lock() = true;
        *self.status.lock() = WakeStatus::Paused;
        if let Some(handle) = self.thread_handle.lock().take() {
            let _ = handle.join();
        }
        *self.events_tx.lock() = None;
        tracing::info!("wake word: background thread stopped");
    }

    /// Приостановить wake word (например, когда идёт диктовка).
    pub fn pause(&self) {
        *self.status.lock() = WakeStatus::Paused;
        tracing::debug!("wake word: paused");
    }

    /// Возобновить wake word после паузы.
    pub fn resume(&self) {
        *self.status.lock() = WakeStatus::Listening;
        tracing::debug!("wake word: resumed");
    }
}

impl Default for WakeWordDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Главный цикл wake word: захват → VAD → whisper → проверка фразы.
fn wake_word_loop<F>(
    stt: Arc<SttEngine>,
    audio_device_id: Option<String>,
    config: WakeWordConfig,
    status: Arc<Mutex<WakeStatus>>,
    stop_flag: Arc<Mutex<bool>>,
    rx: Receiver<WakeEvent>,
    on_event: F,
) where
    F: Fn(WakeEvent) + Send + 'static,
{
    tracing::info!(
        "wake word loop: phrase='{}', chunk_ms={}",
        config.phrase,
        config.chunk_ms
    );

    // Рабочий буфер для захвата одного чанка.
    let chunk_samples = (16_000.0 * config.chunk_ms as f32 / 1000.0) as usize;

    loop {
        // Проверка остановки.
        if *stop_flag.lock() {
            tracing::info!("wake word: stop flag set, exiting loop");
            break;
        }

        // Дрейним канал (на случай если кто-то слал события).
        let _ = rx.try_recv();

        // Пропускаем если на паузе (диктовка идёт).
        let cur_status = *status.lock();
        if cur_status == WakeStatus::Paused || cur_status == WakeStatus::Triggered {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        // Захватываем чанк аудио.
        *status.lock() = WakeStatus::Listening;
        let samples = match capture_chunk(&audio_device_id, chunk_samples) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("wake word: capture_chunk failed: {e}");
                on_event(WakeEvent::Error(e.to_string()));
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };

        if samples.is_empty() {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }

        // VAD: пропускаем тишину (экономим CPU).
        let vad_result = vad::detect_with_threshold(&samples, config.vad_threshold);
        if !vad_result.has_speech {
            // Тишина — не гоняем whisper.
            continue;
        }

        // Транскрибируем чанк.
        *status.lock() = WakeStatus::Processing;
        let started = std::time::Instant::now();
        let transcript = match stt.transcribe(&samples, "ru") {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("wake word: transcribe failed: {e}");
                *status.lock() = WakeStatus::Listening;
                continue;
            }
        };
        let elapsed = started.elapsed().as_secs_f32();

        let text_lower = transcript.text.to_lowercase();
        let text_lower = text_lower.trim();

        tracing::debug!(
            "wake word: chunk transcribed in {:.2}s: '{:?}'",
            elapsed,
            transcript.text
        );

        // Проверяем фразу-триггер.
        if !text_lower.is_empty() && text_lower.contains(&config.phrase) {
            tracing::info!(
                "🎯 WAKE WORD DETECTED: '{:?}' contains '{}'",
                transcript.text,
                config.phrase
            );
            *status.lock() = WakeStatus::Triggered;
            on_event(WakeEvent::Detected {
                transcription: transcript.text.clone(),
            });

            // Ждём, пока pipeline не сбросит статус (через resume()).
            // Это блокирует цикл, пока идёт диктовка.
            while *status.lock() == WakeStatus::Triggered {
                if *stop_flag.lock() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        } else {
            *status.lock() = WakeStatus::Listening;
        }
    }

    tracing::info!("wake word loop exited");
}

/// Захватывает ровно `n` сэмплов (16 кГц mono i16) с микрофона.
/// Блокирующий вызов: ждём пока наберётся нужное количество.
fn capture_chunk(
    audio_device_id: &Option<String>,
    n: usize,
) -> AppResult<Vec<i16>> {
    use std::sync::Arc;
    use parking_lot::Mutex;

    let collected: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let writer = collected.clone();

    let stream = crate::audio::AudioCapture::start(
        audio_device_id.as_deref(),
        move |chunk: &[i16]| {
            let mut w = writer.lock();
            if w.len() < n {
                let needed = n - w.len();
                let take = chunk.len().min(needed);
                w.extend_from_slice(&chunk[..take]);
            }
        },
    )?;

    // Ждём, пока наберётся нужное количество сэмплов.
    // chunk_ms = n / 16000 * 1000. Для 1500 мс = 24000 сэмплов.
    let wait_ms = (n as f32 / 16_000.0 * 1000.0) as u64 + 200; // +200 мс запас
    let deadline = std::time::Instant::now() + Duration::from_millis(wait_ms);

    loop {
        if collected.lock().len() >= n {
            break;
        }
        if std::time::Instant::now() > deadline {
            tracing::warn!(
                "wake word: chunk capture timeout (got {}/{})",
                collected.lock().len(),
                n
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // Дропаем stream — это остановит захват.
    drop(stream);

    let mut samples = collected.lock().clone();
    samples.truncate(n);
    Ok(samples)
}
