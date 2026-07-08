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

use crossbeam_channel::{unbounded, Receiver, Sender};
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
    /// Загружать ли отдельную base-модель для wake word, или использовать основную.
    pub use_tiny_model: bool,
    /// Пауза после срабатывания wake word (мс). Защищает от повторных
    /// срабатываний на эхо/щелчки, пока идёт диктовка или вставка текста.
    pub cooldown_ms: u64,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            phrase: "эй ассистент".to_string(),
            chunk_ms: 1500,
            vad_threshold: 0.015,
            use_tiny_model: true,
            cooldown_ms: 3500,
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
    // Нормализуем фразу один раз: lowercase, без пунктуации, без лишних пробелов.
    // Сравнение ниже ведётся по нормализованным строкам, поэтому запятая в
    // «Эй, ассистент» не мешает матчу (равно как и регистр/пунктуация в выводе whisper).
    let phrase_norm = normalize_phrase(&config.phrase);
    tracing::info!(
        "wake word loop: phrase='{}' (normalized='{}'), chunk_ms={}",
        config.phrase,
        phrase_norm,
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
        tracing::debug!(
            "wake word: chunk level={:.4} (threshold {:.4}), has_speech={}",
            chunk_rms(&samples),
            config.vad_threshold,
            vad_result.has_speech
        );
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

        let transcript_norm = normalize_phrase(&transcript.text);

        tracing::debug!(
            "wake word: chunk transcribed in {:.2}s: '{:?}' (normalized='{}')",
            elapsed,
            transcript.text,
            transcript_norm
        );

        // Нечёткий матч: base-модель на русском тоже искажает слова
        // («Эй, ассистент» → «Бег, ассистин»), поэтому точного contains
        // недостаточно. Сравниваем нормализованные строки по словам через
        // Левенштейна: допускаем ~30% ошибок на каждое слово.
        const MATCH_TOLERANCE: f32 = 0.3;
        if !transcript_norm.is_empty()
            && phrase_matches(&transcript_norm, &phrase_norm, MATCH_TOLERANCE)
        {
            tracing::info!(
                "🎯 WAKE WORD DETECTED: '{:?}' (normalized='{}') matches '{}'",
                transcript.text,
                transcript_norm,
                phrase_norm
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

            // Дебаунс: N мс после срабатывания не слушаем повторно,
            // чтобы не поймать эхо/щелчки от вставки текста.
            if !*stop_flag.lock() {
                tracing::debug!("wake word: cooldown {} ms", config.cooldown_ms);
                std::thread::sleep(Duration::from_millis(config.cooldown_ms));
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

/// Нормированный RMS уровня чанка (0.0..1.0) — для диагностического лога.
fn chunk_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: i64 = samples.iter().map(|&s| (s as i64) * (s as i64)).sum();
    ((sum_sq as f32 / samples.len() as f32).sqrt()) / i16::MAX as f32
}

/// Нормализация строки для сравнения wake-фразы и транскрипта:
///   - lowercase (unicode-aware),
///   - выкидываем пунктуацию,
///   - выкидываем whisper-теги в квадратных/круглых скобках
///     («[музыка]», «(смех)», «(плач)» и т.п.),
///   - сжимаем повторные пробелы, trim.
///
/// Применяется и к фразе («Эй, ассистент» → «эй ассистент»), и к выводу whisper.
fn normalize_phrase(s: &str) -> String {
    let lower = s.to_lowercase();

    let mut out = String::with_capacity(lower.len());
    let mut in_brackets: Option<char> = None; // '[', '('
    for ch in lower.chars() {
        match in_brackets {
            // Внутри тега — копим, пока не закроется скобка.
            Some(open) => {
                let closes = match open {
                    '[' => ch == ']',
                    '(' => ch == ')',
                    _ => true,
                };
                if closes {
                    in_brackets = None;
                }
                // Содержимое тега выбрасываем.
            }
            None => {
                // Открывающая скобка — начинаем тег.
                if ch == '[' || ch == '(' {
                    in_brackets = Some(ch);
                    continue;
                }
                // Оставляем только буквы/цифры и пробелы.
                if ch.is_alphanumeric() {
                    out.push(ch);
                } else if ch.is_whitespace() {
                    // Сжимаем пробелы: не добавляем два подряд.
                    if !out.ends_with(' ') {
                        out.push(' ');
                    }
                }
                // Прочая пунктуация (запятая, точка, тире…) — выкидываем.
            }
        }
    }
    out.trim().to_string()
}

/// Классическое расстояние Левенштейна по unicode-символам.
/// Нужно, чтобы допускать искажения вывода whisper-tiny
/// («ассистент» vs «ассистин»).
fn levenshtein_chars(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev = (0..=b.len()).collect::<Vec<usize>>();
    let mut curr = vec![0usize; b.len() + 1];

    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Нечёткий матч: каждое «значимое» слово фразы (> 3 символов, чтобы не
/// опираться на ненадёжное короткое «эй») должно найтись среди слов
/// транскрипта с допустимым количеством ошибок (`tolerance` — доля длины
/// слова). Для «ассистент» (9 симв.) при tolerance=0.3 допускается до 3
/// ошибок → «ассистин» (dist 1) матчится.
fn phrase_matches(transcript_norm: &str, phrase_norm: &str, tolerance: f32) -> bool {
    let phrase_words: Vec<&str> = phrase_norm
        .split_whitespace()
        .filter(|w| w.chars().count() > 3)
        .collect();
    // Если в фразе нет значимых слов — матч нельзя считать надёжным.
    if phrase_words.is_empty() {
        return false;
    }

    let transcript_words: Vec<&str> = transcript_norm.split_whitespace().collect();
    if transcript_words.is_empty() {
        return false;
    }

    phrase_words.iter().all(|pw| {
        let pw_len = pw.chars().count();
        let max_dist = ((pw_len as f32) * tolerance).ceil() as usize;
        transcript_words
            .iter()
            .any(|tw| levenshtein_chars(pw, tw) <= max_dist)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_tags() {
        assert_eq!(normalize_phrase("Эй, ассистент!"), "эй ассистент");
        assert_eq!(normalize_phrase("[Музыка]"), "");
        assert_eq!(normalize_phrase("Привет (смех), мир."), "привет мир");
        // Несколько пробелов сжимаются в один.
        assert_eq!(normalize_phrase("а   б"), "а б");
    }

    #[test]
    fn levenshtein_basic() {
        // «ассистент»(9) → «ассистин»(8): замена е→и + удаление финального т = 2 операции.
        assert_eq!(levenshtein_chars("ассистент", "ассистин"), 2);
        assert_eq!(levenshtein_chars("abc", "abc"), 0);
        assert_eq!(levenshtein_chars("kitten", "sitting"), 3);
    }

    #[test]
    fn matches_distorted_transcript() {
        // whisper-tiny искажает «эй ассистент» → «бег ассистин».
        // Значимое слово фразы — «ассистент», оно матчится.
        assert!(phrase_matches("бег ассистин", "эй ассистент", 0.3));
        // Точная фраза тоже матчится.
        assert!(phrase_matches("эй ассистент", "эй ассистент", 0.3));
        // Нерелевантная речь — не матчится.
        assert!(!phrase_matches("сейчас я тебе расскажу", "эй ассистент", 0.3));
    }

    #[test]
    fn does_not_match_on_short_word_only() {
        // Короткое слово «эй» не должно само по себе давать матч.
        assert!(!phrase_matches("эй послушай", "эй ассистент", 0.3));
    }
}
