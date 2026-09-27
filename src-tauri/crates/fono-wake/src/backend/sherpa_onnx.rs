use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::audio_source::AudioHub;

use crossbeam_channel::{bounded, Sender};

use parking_lot::Mutex;

use crate::callback::CallbackSlot;
use crate::config::WakeWordConfig;
use crate::diag::{self, Diagnostics, DiagnosticsHandle};
use crate::engine::WakeWordEngine;
use crate::error::{WakeWordError, WakeWordResult};
use crate::event::{capabilities_for_backend, WakeWordCapabilities, WakeWordEvent, WakeWordStatus};
use crate::phrases::sherpa_phrase_to_tokens;
use crate::WakeCallback;

/// Fixed keyword model layout used by Fono.
///
/// The user downloads `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`
/// into the configured `model_dir`.
pub struct SherpaOnnxBackend {
    config: WakeWordConfig,
    audio_hub: AudioHub,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    sender: Mutex<Option<Sender<Vec<f32>>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    callback: CallbackSlot,
    diag: DiagnosticsHandle,
}

impl SherpaOnnxBackend {
    pub fn new(config: WakeWordConfig, audio_hub: AudioHub) -> Self {
        Self {
            config,
            audio_hub,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            sender: Mutex::new(None),
            thread: Mutex::new(None),
            callback: CallbackSlot::default(),
            diag: diag::new_handle(),
        }
    }

    fn model_dir(&self) -> PathBuf {
        self.config.model_dir.clone()
    }

    fn expected_files(&self) -> [(PathBuf, &'static str); 4] {
        let dir = self.model_dir();
        [
            (
                dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx"),
                "encoder",
            ),
            (
                dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx"),
                "decoder",
            ),
            (
                dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx"),
                "joiner",
            ),
            (dir.join("tokens.txt"), "tokens"),
        ]
    }

    fn first_missing_file(&self) -> Option<PathBuf> {
        for (path, _name) in self.expected_files() {
            if !path.is_file() {
                return Some(path);
            }
        }
        None
    }
}

impl WakeWordEngine for SherpaOnnxBackend {
    fn start(&mut self, callback: WakeCallback) -> WakeWordResult<()> {
        self.callback.set(callback);

        if let Some(path) = self.first_missing_file() {
            notify(
                &self.callback,
                WakeWordEvent::MissingModel {
                    path: path.to_string_lossy().into_owned(),
                },
                Some(&self.diag),
            );
            *self.status.lock() = WakeWordStatus::MissingModel;
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

        let config = self.config.clone();
        let status = self.status.clone();
        let running = self.running.clone();
        let paused = self.paused.clone();
        let cb = self.callback.clone();
        let diag = self.diag.clone();
        let audio_hub = self.audio_hub.clone();

        diag::set_running(&diag, true);
        diag::set_paused(&diag, false);

        let (tx, rx) = bounded::<Vec<f32>>(400);
        *self.sender.lock() = Some(tx.clone());

        let runtime = SpotterRuntime {
            config,
            audio_hub,
            running,
            paused,
            status,
            callback: cb,
            diag,
        };
        let thread = thread::spawn(move || {
            if let Err(e) = run_spotter(runtime, tx, rx) {
                tracing::error!("fono-wake sherpa: spotter thread ended: {e}");
            }
        });
        *self.thread.lock() = Some(thread);

        Ok(())
    }

    fn stop(&mut self) -> WakeWordResult<()> {
        self.running.store(false, Ordering::SeqCst);
        *self.sender.lock() = None;
        if let Some(thread) = self.thread.lock().take() {
            let _ = thread.join();
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
        Ok(())
    }

    fn resume(&mut self) -> WakeWordResult<()> {
        self.paused.store(false, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Listening;
        diag::set_paused(&self.diag, false);
        Ok(())
    }

    fn status(&self) -> WakeWordStatus {
        *self.status.lock()
    }

    fn capabilities(&self) -> WakeWordCapabilities {
        capabilities_for_backend(self.config.backend)
    }

    fn diagnostics(&self) -> Option<Diagnostics> {
        let mut d = self.diag.lock().data.clone();
        d.running = self.running.load(Ordering::SeqCst);
        d.paused = self.paused.load(Ordering::SeqCst);
        Some(d)
    }
}

impl Drop for SherpaOnnxBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct SpotterRuntime {
    config: WakeWordConfig,
    audio_hub: AudioHub,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    status: Arc<Mutex<WakeWordStatus>>,
    callback: CallbackSlot,
    diag: DiagnosticsHandle,
}

fn run_spotter(
    runtime: SpotterRuntime,
    tx: Sender<Vec<f32>>,
    rx: crossbeam_channel::Receiver<Vec<f32>>,
) -> WakeWordResult<()> {
    let SpotterRuntime {
        config,
        audio_hub,
        running,
        paused,
        status,
        callback,
        diag,
    } = runtime;
    let dir = config.model_dir;
    let encoder = dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let decoder = dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let joiner = dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    let tokens = dir.join("tokens.txt");

    let mut spotter_config = sherpa_onnx::KeywordSpotterConfig::default();
    spotter_config.feat_config.sample_rate = config.sample_rate as i32;
    spotter_config.feat_config.feature_dim = 80;
    spotter_config.model_config.transducer.encoder = Some(encoder.to_string_lossy().into_owned());
    spotter_config.model_config.transducer.decoder = Some(decoder.to_string_lossy().into_owned());
    spotter_config.model_config.transducer.joiner = Some(joiner.to_string_lossy().into_owned());
    spotter_config.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
    spotter_config.model_config.num_threads = 2;
    spotter_config.model_config.provider = Some("cpu".into());
    spotter_config.keywords_threshold = config.threshold.clamp(0.0, 1.0);
    spotter_config.keywords_score = map_sensitivity(config.sensitivity);
    let keywords = sherpa_phrase_to_tokens(&config.phrase)?;
    tracing::info!(
        phrase = %config.phrase,
        keywords = %keywords.trim_end(),
        threshold = spotter_config.keywords_threshold,
        score = spotter_config.keywords_score,
        "fono-wake sherpa: creating keyword spotter"
    );
    spotter_config.keywords_buf = Some(keywords);

    let spotter = match sherpa_onnx::KeywordSpotter::create(&spotter_config) {
        Some(s) => s,
        None => {
            let msg = "failed to create keyword spotter".to_string();
            notify(
                &callback,
                WakeWordEvent::Error {
                    message: msg.clone(),
                },
                Some(&diag),
            );
            diag::record_event(&diag, "Error");
            *status.lock() = WakeWordStatus::Off;
            return Err(WakeWordError::ModelLoad(msg));
        }
    };

    let mut stream = spotter.create_stream();

    tracing::info!("fono-wake sherpa: model loaded");
    notify(&callback, WakeWordEvent::Listening, Some(&diag));
    diag::record_event(&diag, "Listening");
    *status.lock() = WakeWordStatus::Listening;

    // Start microphone capture and feed f32 samples into the processing loop.
    let capture_tx = tx.clone();
    let audio_diag = diag.clone();
    let _subscription = audio_hub.subscribe(
        config.audio_device_id.as_deref(),
        config.sample_rate,
        move |frames: &[i16]| {
            let samples: Vec<f32> = frames.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
            diag::update_audio_level(&audio_diag, &samples);
            if capture_tx.try_send(samples).is_err() {
                tracing::debug!("fono-wake sherpa: audio channel full, dropping chunk");
            }
        },
    )?;

    let cooldown = Duration::from_millis(config.cooldown_ms);
    let mut last_detection: Option<Instant> = None;
    // Do not feed an unbounded amount of idle background into the KWS stream:
    // the GigaSpeech decoder stops detecting after prolonged silence. Keep a
    // short pre-roll and open bounded, fresh streams only for speech sessions.
    let vad_threshold = config.vad_threshold.max(0.001);
    let pre_roll_limit = config.sample_rate as usize / 2;
    let session_limit = config.sample_rate as usize * 6;
    let mut pre_roll = VecDeque::<f32>::with_capacity(pre_roll_limit);
    let mut session_active = false;
    let mut session_samples = 0_usize;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                if paused.load(Ordering::SeqCst) {
                    continue;
                }

                if !session_active {
                    pre_roll.extend(chunk.iter().copied());
                    while pre_roll.len() > pre_roll_limit {
                        pre_roll.pop_front();
                    }
                    let rms = (chunk.iter().map(|sample| sample * sample).sum::<f32>()
                        / chunk.len().max(1) as f32)
                        .sqrt();
                    if rms < vad_threshold {
                        continue;
                    }

                    stream = spotter.create_stream();
                    let buffered: Vec<f32> = pre_roll.drain(..).collect();
                    session_samples = buffered.len();
                    stream.accept_waveform(config.sample_rate as i32, &buffered);
                    session_active = true;
                    tracing::debug!("fono-wake sherpa: speech session started (rms={rms:.4})");
                } else {
                    stream.accept_waveform(config.sample_rate as i32, &chunk);
                    session_samples += chunk.len();
                }

                while spotter.is_ready(&stream) {
                    spotter.decode(&stream);
                    if let Some(result) = spotter.get_result(&stream) {
                        let keyword = result.keyword.trim();
                        let json = result.json.trim();
                        if !keyword.is_empty() {
                            tracing::debug!(
                                "fono-wake sherpa: result keyword='{keyword}' json='{json}'"
                            );
                            diag::record_result(&diag, keyword, json);
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
                                        pre_roll: Vec::new(),
                                    },
                                    Some(&diag),
                                );
                            }
                            stream = spotter.create_stream();
                            session_active = false;
                            session_samples = 0;
                            pre_roll.clear();
                            notify(&callback, WakeWordEvent::Listening, Some(&diag));
                            break;
                        }
                    }
                }

                if session_active && session_samples >= session_limit {
                    stream = spotter.create_stream();
                    session_active = false;
                    session_samples = 0;
                    pre_roll.clear();
                    tracing::debug!("fono-wake sherpa: speech session rotated");
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    *status.lock() = WakeWordStatus::Off;
    Ok(())
}

/// Map 0..1 sensitivity to a keywords_score that feels reasonable.
pub(crate) fn map_sensitivity(s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    0.5 + s * 3.5
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

fn notify(callback: &CallbackSlot, event: WakeWordEvent, diag: Option<&DiagnosticsHandle>) {
    if let Some(d) = diag {
        diag::record_event(d, event_name(&event));
    }
    callback.notify(event);
}

#[cfg(test)]
mod tests {
    use super::map_sensitivity;

    #[test]
    fn sensitivity_is_clamped_before_mapping() {
        assert_eq!(map_sensitivity(-1.0), 0.5);
        assert_eq!(map_sensitivity(0.0), 0.5);
        assert_eq!(map_sensitivity(1.0), 4.0);
        assert_eq!(map_sensitivity(2.0), 4.0);
    }
}
