pub use super::whisper_helpers::test_with_samples;
use super::whisper_helpers::{
    gpu_backend_compiled, has_speech, load_model, normalize_phrase, phrase_matches, transcribe,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::audio_source::AudioHub;
use crate::callback::CallbackSlot;
use crate::config::WakeWordConfig;
use crate::diag::{self, Diagnostics, DiagnosticsHandle};
use crate::engine::WakeWordEngine;
use crate::error::{WakeWordError, WakeWordResult};
use crate::event::{capabilities_for_backend, WakeWordCapabilities, WakeWordEvent, WakeWordStatus};
use crate::WakeCallback;

/// Whisper-based wake word detector for phrases that tiny open-vocabulary KWS
/// models cannot recognize reliably.
pub struct WhisperExperimentalBackend {
    config: WakeWordConfig,
    audio_hub: AudioHub,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    callback: CallbackSlot,
    diag: DiagnosticsHandle,
}

impl WhisperExperimentalBackend {
    pub fn new(config: WakeWordConfig, audio_hub: AudioHub) -> Self {
        Self {
            config,
            audio_hub,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            callback: CallbackSlot::default(),
            diag: diag::new_handle(),
        }
    }
}

impl WakeWordEngine for WhisperExperimentalBackend {
    fn start(&mut self, callback: WakeCallback) -> WakeWordResult<()> {
        self.callback.set(callback);
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
        let audio_hub = self.audio_hub.clone();
        let handle = thread::spawn(move || {
            let error_callback = callback.clone();
            let error_diag = diag.clone();
            if let Err(error) = whisper_loop(
                config,
                audio_hub,
                running,
                paused,
                status,
                callback,
                diag.clone(),
            ) {
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

    fn capabilities(&self) -> WakeWordCapabilities {
        capabilities_for_backend(self.config.backend)
    }

    fn diagnostics(&self) -> Option<Diagnostics> {
        let mut diagnostics = self.diag.lock().data.clone();
        diagnostics.running = self.running.load(Ordering::SeqCst);
        diagnostics.paused = self.paused.load(Ordering::SeqCst);
        Some(diagnostics)
    }
}

impl Drop for WhisperExperimentalBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn whisper_loop(
    config: WakeWordConfig,
    audio_hub: AudioHub,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    status: Arc<Mutex<WakeWordStatus>>,
    callback: CallbackSlot,
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

    let _subscription = audio_hub.subscribe(
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
                        audio_cursor: None,
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

fn notify(callback: &CallbackSlot, event: WakeWordEvent, diagnostics: Option<&DiagnosticsHandle>) {
    if let Some(diagnostics) = diagnostics {
        diag::record_event(diagnostics, event_name(&event));
    }
    callback.notify(event);
}
