//! Energy-based Voice Activity Detection (VAD).
//!
//! Простой, без внешних зависимостей (silero-vad тянем позже, если понадобится).
//! Алгоритм:
//!   1. Разбиваем аудио на окна по ~30 мс.
//!   2. Считаем RMS каждого окна.
//!   3. Помечаем окна как "речь" если RMS > threshold.
//!   4. trim_silence: отрезаем тишину в начале и в конце.
//!
//! Используется в stop_recording, чтобы:
//!   - whisper.cpp получал только полезное аудио (ускорение 1.5-2x).
//!   - игнорировать случайные щелчки/шумы.

const FRAME_MS: f32 = 30.0;
const DEFAULT_THRESHOLD: f32 = 0.012; // RMS-порог (~-38 dBFS) — подбирается под микрофон.
const PADDING_MS: f32 = 150.0; // Паддинг вокруг речи, чтобы не обрезать согласные.

/// Результат детекции речи в буфере.
#[derive(Debug, Clone)]
pub struct VadResult {
    /// Индекс первого сэмпла речи (с учётом padding).
    pub start_sample: usize,
    /// Индекс последнего сэмпла речи (с учётом padding).
    pub end_sample: usize,
    /// Обнаружена ли речь вообще.
    pub has_speech: bool,
    /// Сколько сэмплов было отрезано (для логирования).
    pub trimmed_samples: usize,
}

/// Анализирует буфер и находит границы речи.
pub fn detect(samples: &[i16]) -> VadResult {
    detect_with_threshold(samples, DEFAULT_THRESHOLD)
}

pub fn detect_with_threshold(samples: &[i16], threshold: f32) -> VadResult {
    let n = samples.len();
    if n == 0 {
        return VadResult {
            start_sample: 0,
            end_sample: 0,
            has_speech: false,
            trimmed_samples: 0,
        };
    }

    let frame_size = (16_000.0 * FRAME_MS / 1000.0) as usize; // ~480 сэмплов
    let padding = (16_000.0 * PADDING_MS / 1000.0) as usize; // ~2400 сэмплов
    let n_frames = n.div_ceil(frame_size);

    // Считаем RMS каждого окна и помечаем речь.
    let mut frame_is_speech = vec![false; n_frames];
    for (i, chunk) in samples.chunks(frame_size).enumerate() {
        let sum_sq: i64 = chunk.iter().map(|&s| (s as i64) * (s as i64)).sum();
        let rms = (sum_sq as f32 / chunk.len() as f32).sqrt() / i16::MAX as f32;
        if rms > threshold {
            frame_is_speech[i] = true;
        }
    }

    // Находим первую/последнюю речь.
    let first_speech_frame = frame_is_speech.iter().position(|&x| x);
    let last_speech_frame = frame_is_speech.iter().rposition(|&x| x);

    let (start, end) = match (first_speech_frame, last_speech_frame) {
        (Some(first), Some(last)) => {
            let s = first.saturating_mul(frame_size).saturating_sub(padding);
            let e = ((last + 1).saturating_mul(frame_size)).min(n) + padding;
            (s.min(n), e.min(n))
        }
        _ => (0, 0), // речи нет
    };

    let has_speech = first_speech_frame.is_some();
    let trimmed = start + (n.saturating_sub(end));

    VadResult {
        start_sample: start,
        end_sample: end,
        has_speech,
        trimmed_samples: trimmed,
    }
}

/// Возвращает под-срез сэмплов, содержащий только речь (тишина обрезана).
/// Если речь не обнаружена — возвращает исходный буфер как есть
/// (вдруг VAD ошибся и пользователь действительно что-то сказал).
pub fn trim_silence(samples: &[i16]) -> Vec<i16> {
    let result = detect(samples);
    if !result.has_speech || result.end_sample <= result.start_sample {
        tracing::debug!("VAD: речь не обнаружена, возвращаем исходный буфер");
        return samples.to_vec();
    }
    let trimmed = &samples[result.start_sample..result.end_sample];
    tracing::info!(
        "VAD: обрезано {} сэмплов тишины (осталось {} из {}, {:.1}% сохранено)",
        result.trimmed_samples,
        trimmed.len(),
        samples.len(),
        100.0 * trimmed.len() as f32 / samples.len() as f32
    );
    trimmed.to_vec()
}
