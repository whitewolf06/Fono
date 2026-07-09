use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use whisper_rs::{SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::WakeWordConfig;
use crate::engine::WakeWordEngine;
use crate::error::{WakeWordError, WakeWordResult};
use crate::event::{WakeWordEvent, WakeWordStatus};

/// Whisper-based wake word detector.
///
/// **Experimental / deprecated**: kept for compatibility. The recommended
/// backend is `SherpaOnnx` because it is orders of magnitude faster for the
/// wake-word task.
pub struct WhisperExperimentalBackend {
    config: WakeWordConfig,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
}

impl WhisperExperimentalBackend {
    pub fn new(config: WakeWordConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    fn model_path(&self) -> PathBuf {
        // For this backend `model_dir` is interpreted as the path to the GGML model file.
        self.config.model_dir.clone()
    }
}

impl WakeWordEngine for WhisperExperimentalBackend {
    fn start(&mut self, callback: Box<dyn Fn(WakeWordEvent) + Send>) -> WakeWordResult<()> {
        *self.callback.lock() = Some(callback);

        let model_path = self.model_path();
        if !model_path.is_file() {
            *self.status.lock() = WakeWordStatus::MissingModel;
            notify(
                &self.callback,
                WakeWordEvent::MissingModel {
                    path: model_path.to_string_lossy().into_owned(),
                },
            );
            return Err(WakeWordError::ModelNotFound(model_path));
        }

        notify(&self.callback, WakeWordEvent::ModelLoading);
        *self.status.lock() = WakeWordStatus::Loading;

        self.running.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);

        let config = self.config.clone();
        let status = self.status.clone();
        let running = self.running.clone();
        let paused = self.paused.clone();
        let cb = self.callback.clone();
        let device_id = config.audio_device_id.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = whisper_loop(model_path, device_id, config, running, paused, status, cb) {
                tracing::error!("fono-wake whisper: loop ended: {e}");
            }
        });

        *self.thread.lock() = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> WakeWordResult<()> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().take() {
            let _ = handle.join();
        }
        *self.status.lock() = WakeWordStatus::Off;
        Ok(())
    }

    fn pause(&mut self) -> WakeWordResult<()> {
        self.paused.store(true, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Paused;
        Ok(())
    }

    fn resume(&mut self) -> WakeWordResult<()> {
        self.paused.store(false, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Listening;
        Ok(())
    }

    fn status(&self) -> WakeWordStatus {
        *self.status.lock()
    }
}

fn whisper_loop(
    model_path: PathBuf,
    device_id: Option<String>,
    config: WakeWordConfig,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    status: Arc<Mutex<WakeWordStatus>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
) -> WakeWordResult<()> {
    let ctx = load_model(&model_path)?;
    let ctx = Arc::new(ctx);

    notify(&callback, WakeWordEvent::Listening);
    *status.lock() = WakeWordStatus::Listening;

    let phrase_norm = normalize_phrase(&config.phrase);
    let chunk_ms = 1_500_u64;
    let chunk_samples = (16_000.0 * chunk_ms as f32 / 1000.0) as usize;
    let vad_threshold = config.threshold.max(0.001);
    let cooldown = Duration::from_millis(config.cooldown_ms);
    let mut last_detection: Option<Instant> = None;

    while running.load(Ordering::SeqCst) {
        if paused.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        *status.lock() = WakeWordStatus::Listening;
        let samples = match capture_chunk(&device_id, chunk_samples) {
            Ok(s) => s,
            Err(e) => {
                notify(&callback, WakeWordEvent::Error { message: e.to_string() });
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };

        if samples.is_empty() {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        if !has_speech(&samples, vad_threshold) {
            continue;
        }

        *status.lock() = WakeWordStatus::Processing;
        let transcript = match transcribe(&ctx, &samples) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("fono-wake whisper: transcribe failed: {e}");
                continue;
            }
        };

        let transcript_norm = normalize_phrase(&transcript);
        if !transcript_norm.is_empty() && phrase_matches(&transcript_norm, &phrase_norm, 0.3) {
            let now = Instant::now();
            let allow = last_detection
                .map(|t| now.duration_since(t) >= cooldown)
                .unwrap_or(true);
            if allow {
                last_detection = Some(now);
                notify(
                    &callback,
                    WakeWordEvent::Detected {
                        phrase: config.phrase.clone(),
                    },
                );
                thread::sleep(cooldown);
            }
        }
    }

    *status.lock() = WakeWordStatus::Off;
    Ok(())
}

fn load_model(path: &std::path::Path) -> WakeWordResult<WhisperContext> {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(false);
    WhisperContext::new_with_params(path, params)
        .map_err(|e| WakeWordError::ModelLoad(format!("WhisperContext: {e}")))
}

fn transcribe(ctx: &WhisperContext, samples: &[i16]) -> WakeWordResult<String> {
    let pcm: Vec<f32> = samples.iter().map(|&s| s as f32 / i16::MAX as f32).collect();

    let mut params = whisper_rs::FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("ru"));
    params.set_n_threads(2);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_no_timestamps(true);

    let mut state = ctx
        .create_state()
        .map_err(|e| WakeWordError::Backend(format!("create_state: {e}")))?;
    state
        .full(params, &pcm)
        .map_err(|e| WakeWordError::Backend(format!("full: {e}")))?;

    let n = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n {
        if let Some(seg) = state.get_segment(i) {
            if let Ok(t) = seg.to_str_lossy() {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(trimmed);
                }
            }
        }
    }
    Ok(text.trim().to_string())
}

fn capture_chunk(device_id: &Option<String>, n: usize) -> WakeWordResult<Vec<i16>> {
    let host = cpal::default_host();
    let device = match device_id {
        Some(id) => host
            .input_devices()
            .map_err(|e| WakeWordError::Audio(e.to_string()))?
            .find(|d| d.name().ok().as_deref() == Some(id))
            .ok_or_else(|| WakeWordError::Audio(format!("device not found: {id}")))?,
        None => host
            .default_input_device()
            .ok_or_else(|| WakeWordError::Audio("no default input device".into()))?,
    };

    let config_range = device
        .default_input_config()
        .map_err(|e| WakeWordError::Audio(e.to_string()))?;
    let sample_format = config_range.sample_format();
    let mut stream_config: cpal::StreamConfig = config_range.into();
    stream_config.channels = 1;
    let sample_rate = stream_config.sample_rate.0 as f32;

    let collected: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let writer = collected.clone();

    let stream = device.build_input_stream_raw(
        &stream_config,
        sample_format,
        move |data: &cpal::Data, _: &_| {
            let mut w = writer.lock();
            if w.len() >= n {
                return;
            }
            let chunk = convert_to_i16(data, sample_format);
            let mono = if stream_config.channels == 1 {
                chunk
            } else {
                chunk
                    .chunks(stream_config.channels as usize)
                    .map(|c| (c.iter().map(|&s| s as i64).sum::<i64>() / c.len() as i64) as i16)
                    .collect()
            };
            let resampled = if (sample_rate - 16_000.0).abs() > 1.0 {
                linear_resample(&mono, sample_rate, 16_000.0)
            } else {
                mono
            };
            let needed = n - w.len();
            let take = resampled.len().min(needed);
            w.extend_from_slice(&resampled[..take]);
        },
        |err| tracing::error!("fono-wake whisper capture error: {err}"),
        None,
    )?;

    stream.play()?;

    let wait_ms = (n as f32 / 16_000.0 * 1000.0) as u64 + 200;
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    while collected.lock().len() < n {
        if Instant::now() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    drop(stream);
    let mut samples = collected.lock().clone();
    samples.truncate(n);
    Ok(samples)
}

fn has_speech(samples: &[i16], threshold: f32) -> bool {
    if samples.is_empty() {
        return false;
    }
    let sum_sq: i64 = samples.iter().map(|&s| (s as i64) * (s as i64)).sum();
    let rms = ((sum_sq as f32 / samples.len() as f32).sqrt()) / i16::MAX as f32;
    rms >= threshold
}

fn normalize_phrase(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_brackets: Option<char> = None;
    for ch in lower.chars() {
        match in_brackets {
            Some(open) => {
                let closes = match open {
                    '[' => ch == ']',
                    '(' => ch == ')',
                    _ => true,
                };
                if closes {
                    in_brackets = None;
                }
            }
            None => {
                if ch == '[' || ch == '(' {
                    in_brackets = Some(ch);
                    continue;
                }
                if ch.is_alphanumeric() {
                    out.push(ch);
                } else if ch.is_whitespace() && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
        }
    }
    out.trim().to_string()
}

fn levenshtein_chars(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
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

fn phrase_matches(transcript_norm: &str, phrase_norm: &str, tolerance: f32) -> bool {
    let phrase_words: Vec<&str> = phrase_norm
        .split_whitespace()
        .filter(|w| w.chars().count() > 3)
        .collect();
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
        transcript_words.iter().any(|tw| levenshtein_chars(pw, tw) <= max_dist)
    })
}

fn convert_to_i16(data: &cpal::Data, format: cpal::SampleFormat) -> Vec<i16> {
    let bytes = data.bytes();
    match format {
        cpal::SampleFormat::I8 => samples_to_i16::<i8, _>(bytes, |s| (s as i16) << 8),
        cpal::SampleFormat::I16 => samples_to_i16::<i16, _>(bytes, |s| s),
        cpal::SampleFormat::I32 => samples_to_i16::<i32, _>(bytes, |s| (s >> 16) as i16),
        cpal::SampleFormat::I64 => samples_to_i16::<i64, _>(bytes, |s| (s >> 48) as i16),
        cpal::SampleFormat::U8 => samples_to_i16::<u8, _>(bytes, |s| (s as i16 - 128) << 8),
        cpal::SampleFormat::U16 => samples_to_i16::<u16, _>(bytes, |s| (s as i32 - 32768) as i16),
        cpal::SampleFormat::U32 => {
            samples_to_i16::<u32, _>(bytes, |s| ((s as i64 - 2_147_483_648) >> 16) as i16)
        }
        cpal::SampleFormat::U64 => samples_to_i16::<u64, _>(bytes, |s| {
            ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16
        }),
        cpal::SampleFormat::F32 => samples_to_i16::<f32, _>(bytes, |s| float_to_i16(s as f64)),
        cpal::SampleFormat::F64 => samples_to_i16::<f64, _>(bytes, float_to_i16),
        _ => Vec::new(),
    }
}

fn samples_to_i16<T: Copy, F>(bytes: &[u8], mut convert: F) -> Vec<i16>
where
    F: FnMut(T) -> i16,
{
    if bytes.is_empty() {
        return Vec::new();
    }
    let _sample_size = std::mem::size_of::<T>();
    let (_, samples, _) = unsafe { bytes.align_to::<T>() };
    samples.iter().copied().map(&mut convert).collect()
}

fn float_to_i16(v: f64) -> i16 {
    (v.clamp(-1.0, 1.0) * i16::MAX as f64) as i16
}

fn linear_resample(input: &[i16], in_rate: f32, out_rate: f32) -> Vec<i16> {
    let ratio = out_rate / in_rate;
    let out_len = ((input.len() as f32) * ratio) as usize;
    let last = input.len().saturating_sub(1);
    (0..out_len)
        .map(|i| {
            let src = i as f32 / ratio;
            let idx = src.floor() as usize;
            let frac = src - idx as f32;
            let a = input[idx.min(last)] as f32;
            let b = input[(idx + 1).min(last)] as f32;
            (a + (b - a) * frac) as i16
        })
        .collect()
}

fn notify(
    callback: &Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    event: WakeWordEvent,
) {
    if let Some(cb) = callback.lock().as_ref() {
        cb(event);
    }
}
