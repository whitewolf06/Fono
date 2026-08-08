use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use whisper_rs::{SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio_source::AudioStream;
use crate::config::WakeWordConfig;
use crate::diag::{self, Diagnostics, DiagnosticsHandle};
use crate::engine::WakeWordEngine;
use crate::error::{WakeWordError, WakeWordResult};
use crate::event::{WakeWordEvent, WakeWordStatus};
use crate::test::WakeWordTestResult;

/// Whisper-based wake word detector for phrases that tiny open-vocabulary KWS
/// models cannot recognize reliably.
pub struct WhisperExperimentalBackend {
    config: WakeWordConfig,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    diag: DiagnosticsHandle,
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
            diag: diag::new_handle(),
        }
    }
}

impl WakeWordEngine for WhisperExperimentalBackend {
    fn start(&mut self, callback: Box<dyn Fn(WakeWordEvent) + Send>) -> WakeWordResult<()> {
        *self.callback.lock() = Some(callback);
        if !self.config.model_dir.is_file() {
            let path = self.config.model_dir.clone();
            *self.status.lock() = WakeWordStatus::MissingModel;
            notify(
                &self.callback,
                WakeWordEvent::MissingModel {
                    path: path.to_string_lossy().into_owned(),
                },
                Some(&self.diag),
            );
            return Err(WakeWordError::ModelNotFound(path));
        }

        notify(
            &self.callback,
            WakeWordEvent::ModelLoading,
            Some(&self.diag),
        );
        *self.status.lock() = WakeWordStatus::Loading;
        self.running.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        diag::set_running(&self.diag, true);
        diag::set_paused(&self.diag, false);

        let config = self.config.clone();
        let status = self.status.clone();
        let running = self.running.clone();
        let paused = self.paused.clone();
        let callback = self.callback.clone();
        let diag = self.diag.clone();
        let handle = thread::spawn(move || {
            let error_callback = callback.clone();
            let error_diag = diag.clone();
            if let Err(error) =
                whisper_loop(config, running, paused, status, callback, diag.clone())
            {
                tracing::error!("fono-wake whisper: loop ended: {error}");
                notify(
                    &error_callback,
                    WakeWordEvent::Error {
                        message: error.to_string(),
                    },
                    Some(&error_diag),
                );
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
        diag::set_running(&self.diag, false);
        diag::set_paused(&self.diag, false);
        Ok(())
    }

    fn pause(&mut self) -> WakeWordResult<()> {
        self.paused.store(true, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Paused;
        diag::set_paused(&self.diag, true);
        notify(&self.callback, WakeWordEvent::Paused, Some(&self.diag));
        Ok(())
    }

    fn resume(&mut self) -> WakeWordResult<()> {
        self.paused.store(false, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Listening;
        diag::set_paused(&self.diag, false);
        notify(&self.callback, WakeWordEvent::Listening, Some(&self.diag));
        Ok(())
    }

    fn status(&self) -> WakeWordStatus {
        *self.status.lock()
    }

    fn diagnostics(&self) -> Option<Diagnostics> {
        let mut diagnostics = self.diag.lock().data.clone();
        diagnostics.running = self.running.load(Ordering::SeqCst);
        diagnostics.paused = self.paused.load(Ordering::SeqCst);
        Some(diagnostics)
    }
}

fn whisper_loop(
    config: WakeWordConfig,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    status: Arc<Mutex<WakeWordStatus>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    diagnostics: DiagnosticsHandle,
) -> WakeWordResult<()> {
    let use_gpu = config.use_gpu && gpu_backend_compiled();
    let context = load_model(&config.model_dir, use_gpu)?;
    let phrase = normalize_phrase(&config.phrase);
    let language = if config.phrase.is_ascii() { "en" } else { "ru" };
    let sample_rate = config.sample_rate as usize;
    let window_limit = sample_rate * 5 / 2;
    let minimum_window = sample_rate;
    let audio = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(window_limit)));
    let audio_writer = audio.clone();
    let audio_diag = diagnostics.clone();

    let _stream = AudioStream::start(
        config.audio_device_id.as_deref(),
        config.sample_rate,
        move |frames| {
            let normalized: Vec<f32> = frames
                .iter()
                .map(|&sample| sample as f32 / i16::MAX as f32)
                .collect();
            diag::update_audio_level(&audio_diag, &normalized);

            let mut buffer = audio_writer.lock();
            buffer.extend(frames.iter().copied());
            while buffer.len() > window_limit {
                buffer.pop_front();
            }
        },
    )?;

    tracing::info!(
        "fono-wake whisper: model loaded (gpu={}, language={language})",
        use_gpu
    );
    *status.lock() = WakeWordStatus::Listening;
    notify(&callback, WakeWordEvent::Listening, Some(&diagnostics));

    let cooldown = Duration::from_millis(config.cooldown_ms);
    // Детектор просыпается раз в 750 мс. Берём хвост его кольцевого буфера,
    // чтобы слова, сказанные сразу после wake phrase, попали в диктовку.
    let pre_roll_samples = sample_rate * 7 / 10;
    let mut last_detection: Option<Instant> = None;
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(750));
        if paused.load(Ordering::SeqCst) {
            audio.lock().clear();
            continue;
        }

        let samples: Vec<i16> = audio.lock().iter().copied().collect();
        if samples.len() < minimum_window || !has_speech(&samples, config.vad_threshold) {
            continue;
        }

        *status.lock() = WakeWordStatus::Processing;
        let transcript = match transcribe(&context, &samples, language) {
            Ok(transcript) => transcript,
            Err(error) => {
                tracing::warn!("fono-wake whisper: transcribe failed: {error}");
                *status.lock() = WakeWordStatus::Listening;
                continue;
            }
        };
        let normalized = normalize_phrase(&transcript);
        if !normalized.is_empty() {
            tracing::debug!("fono-wake whisper: transcript='{normalized}'");
            diag::record_result(&diagnostics, &normalized, &transcript);
        }

        if phrase_matches(&normalized, &phrase) {
            let now = Instant::now();
            let allowed = last_detection
                .map(|last| now.duration_since(last) >= cooldown)
                .unwrap_or(true);
            if allowed {
                last_detection = Some(now);
                let pre_roll = {
                    let buffer = audio.lock();
                    let start = buffer.len().saturating_sub(pre_roll_samples);
                    buffer.iter().skip(start).copied().collect()
                };
                notify(
                    &callback,
                    WakeWordEvent::Detected {
                        phrase: config.phrase.clone(),
                        pre_roll,
                    },
                    Some(&diagnostics),
                );
                audio.lock().clear();
                *status.lock() = WakeWordStatus::Listening;
                continue;
            }
        }

        *status.lock() = WakeWordStatus::Listening;
        notify(&callback, WakeWordEvent::Listening, Some(&diagnostics));
    }

    *status.lock() = WakeWordStatus::Off;
    Ok(())
}

fn load_model(path: &std::path::Path, use_gpu: bool) -> WakeWordResult<WhisperContext> {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(use_gpu);
    WhisperContext::new_with_params(path, params)
        .map_err(|error| WakeWordError::ModelLoad(format!("WhisperContext: {error}")))
}

fn gpu_backend_compiled() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

pub fn test_with_samples(
    config: &WakeWordConfig,
    samples: &[i16],
) -> WakeWordResult<WakeWordTestResult> {
    let started = Instant::now();
    let context = load_model(&config.model_dir, config.use_gpu)?;
    let language = if config.phrase.is_ascii() { "en" } else { "ru" };
    let transcript = transcribe(&context, samples, language)?;
    let normalized = normalize_phrase(&transcript);
    let phrase = normalize_phrase(&config.phrase);
    Ok(WakeWordTestResult {
        detected: phrase_matches(&normalized, &phrase),
        keyword: transcript.clone(),
        json: format!(
            "{{\"transcript\":{},\"normalized\":{}}}",
            serde_json::to_string(&transcript).unwrap_or_else(|_| "\"\"".into()),
            serde_json::to_string(&normalized).unwrap_or_else(|_| "\"\"".into())
        ),
        samples: samples.len(),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn transcribe(ctx: &WhisperContext, samples: &[i16], language: &str) -> WakeWordResult<String> {
    let pcm: Vec<f32> = samples
        .iter()
        .map(|&sample| sample as f32 / i16::MAX as f32)
        .collect();
    let mut params = whisper_rs::FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_n_threads(4);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_no_timestamps(true);

    let mut state = ctx
        .create_state()
        .map_err(|error| WakeWordError::Backend(format!("create_state: {error}")))?;
    state
        .full(params, &pcm)
        .map_err(|error| WakeWordError::Backend(format!("full: {error}")))?;

    let mut text = String::new();
    for index in 0..state.full_n_segments() {
        if let Some(segment) = state.get_segment(index) {
            if let Ok(value) = segment.to_str_lossy() {
                let value = value.trim();
                if !value.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(value);
                }
            }
        }
    }
    Ok(text)
}

fn has_speech(samples: &[i16], threshold: f32) -> bool {
    let threshold = threshold.max(0.001);
    samples.chunks(1_600).any(|window| {
        let sum: i64 = window
            .iter()
            .map(|&sample| (sample as i64) * (sample as i64))
            .sum();
        let rms = (sum as f32 / window.len().max(1) as f32).sqrt() / i16::MAX as f32;
        rms >= threshold
    })
}

fn normalize_phrase(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for character in value.to_lowercase().chars() {
        if character.is_alphanumeric() {
            normalized.push(character);
        } else if character.is_whitespace() && !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    normalized.trim().to_string()
}

fn phrase_matches(transcript: &str, phrase: &str) -> bool {
    if transcript.is_empty() || phrase.is_empty() {
        return false;
    }
    if phrase == "hey fono" {
        let words: Vec<&str> = transcript.split_whitespace().collect();
        let prefix = words
            .iter()
            .any(|word| matches!(*word, "hey" | "she" | "hi" | "хей"));
        let name = words
            .iter()
            .any(|word| matches!(*word, "fono" | "phono" | "phone" | "фоно" | "фона"));
        return prefix && name;
    }
    if phrase == "okay fun" {
        let words: Vec<&str> = transcript.split_whitespace().collect();
        let prefix = words
            .iter()
            .any(|word| matches!(*word, "okay" | "ok" | "okey" | "окей"));
        let name = words
            .iter()
            .any(|word| matches!(*word, "fun" | "fan" | "фан" | "фэн"));
        return prefix && name;
    }
    transcript.contains(phrase)
}

fn event_name(event: &WakeWordEvent) -> &'static str {
    match event {
        WakeWordEvent::Listening => "Listening",
        WakeWordEvent::Paused => "Paused",
        WakeWordEvent::Detected { .. } => "Detected",
        WakeWordEvent::Error { .. } => "Error",
        WakeWordEvent::ModelLoading => "ModelLoading",
        WakeWordEvent::MissingModel { .. } => "MissingModel",
    }
}

fn notify(
    callback: &Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    event: WakeWordEvent,
    diagnostics: Option<&DiagnosticsHandle>,
) {
    if let Some(diagnostics) = diagnostics {
        diag::record_event(diagnostics, event_name(&event));
    }
    if let Some(callback) = callback.lock().as_ref() {
        callback(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_phrase, phrase_matches};

    #[test]
    fn default_phrase_accepts_observed_pronunciation() {
        assert!(phrase_matches("she phono", "hey fono"));
        assert!(phrase_matches("hey fono", "hey fono"));
        assert!(!phrase_matches("phone", "hey fono"));
    }

    #[test]
    fn okay_fun_accepts_common_variants() {
        assert!(phrase_matches("okay fun", "okay fun"));
        assert!(phrase_matches("ok fan", "okay fun"));
        assert!(!phrase_matches("have fun", "okay fun"));
    }

    #[test]
    fn normalization_removes_punctuation() {
        assert_eq!(normalize_phrase(" She, PHONO! "), "she phono");
    }
}
