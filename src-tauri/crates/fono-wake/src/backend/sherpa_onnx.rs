use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::audio_source::AudioStream;

use crossbeam_channel::{bounded, Sender};

use parking_lot::Mutex;

use crate::config::WakeWordConfig;
use crate::diag::{self, Diagnostics, DiagnosticsHandle};
use crate::engine::WakeWordEngine;
use crate::error::{WakeWordError, WakeWordResult};
use crate::event::{WakeWordEvent, WakeWordStatus};

/// Fixed keyword model layout used by Fono.
///
/// The user downloads `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`
/// into the configured `model_dir`.
pub struct SherpaOnnxBackend {
    config: WakeWordConfig,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    sender: Mutex<Option<Sender<Vec<f32>>>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    diag: DiagnosticsHandle,
}

impl SherpaOnnxBackend {
    pub fn new(config: WakeWordConfig) -> Self {
        Self {
            config,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            sender: Mutex::new(None),
            callback: Arc::new(Mutex::new(None)),
            diag: diag::new_handle(),
        }
    }

    fn model_dir(&self) -> PathBuf {
        self.config.model_dir.clone()
    }

    fn expected_files(&self) -> [(PathBuf, &'static str); 4] {
        let dir = self.model_dir();
        [
            (dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx"), "encoder"),
            (dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx"), "decoder"),
            (dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx"), "joiner"),
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
    fn start(&mut self, callback: Box<dyn Fn(WakeWordEvent) + Send>) -> WakeWordResult<()> {
        *self.callback.lock() = Some(callback);

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

        notify(&self.callback, WakeWordEvent::ModelLoading, Some(&self.diag));
        *self.status.lock() = WakeWordStatus::Loading;

        self.running.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);

        let config = self.config.clone();
        let status = self.status.clone();
        let running = self.running.clone();
        let paused = self.paused.clone();
        let cb = self.callback.clone();
        let diag = self.diag.clone();

        diag::set_running(&diag, true);
        diag::set_paused(&diag, false);

        let (tx, rx) = bounded::<Vec<f32>>(400);
        *self.sender.lock() = Some(tx.clone());

        thread::spawn(move || {
            if let Err(e) = run_spotter(config, running, paused, status, tx, rx, cb, diag) {
                tracing::error!("fono-wake sherpa: spotter thread ended: {e}");
            }
        });

        Ok(())
    }

    fn stop(&mut self) -> WakeWordResult<()> {
        self.running.store(false, Ordering::SeqCst);
        *self.sender.lock() = None;
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

    fn diagnostics(&self) -> Option<Diagnostics> {
        let mut d = self.diag.lock().data.clone();
        d.running = self.running.load(Ordering::SeqCst);
        d.paused = self.paused.load(Ordering::SeqCst);
        Some(d)
    }
}

fn run_spotter(
    config: WakeWordConfig,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    status: Arc<Mutex<WakeWordStatus>>,
    tx: Sender<Vec<f32>>,
    rx: crossbeam_channel::Receiver<Vec<f32>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    diag: DiagnosticsHandle,
) -> WakeWordResult<()> {
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
    spotter_config.keywords_buf = Some(phrase_to_tokens(&config.phrase));

    let spotter = match sherpa_onnx::KeywordSpotter::create(&spotter_config) {
        Some(s) => s,
        None => {
            let msg = "failed to create keyword spotter".to_string();
            notify(&callback, WakeWordEvent::Error { message: msg.clone() }, Some(&diag));
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
    let _audio_stream = AudioStream::start(
        config.audio_device_id.as_deref(),
        config.sample_rate,
        move |frames: &[i16]| {
            let samples: Vec<f32> = frames
                .iter()
                .map(|&s| s as f32 / i16::MAX as f32)
                .collect();
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
                    let rms = (chunk
                        .iter()
                        .map(|sample| sample * sample)
                        .sum::<f32>()
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
                    tracing::debug!(
                        "fono-wake sherpa: speech session started (rms={rms:.4})"
                    );
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

pub(crate) fn phrase_to_tokens(phrase: &str) -> String {
    let normalized = phrase.trim().to_ascii_uppercase();

    // For the default English GigaSpeech KWS model the wake phrase is
    // tokenized with the included BPE model. "Fono" is a product name that
    // the acoustic model can interpret as FONO, PHONO, PHONE-O, or FUNO, so
    // keep those pronunciations in one keyword graph. Any matched variant is
    // still reported to the application as the configured wake phrase.
    if normalized.is_empty() || normalized == "HEY FONO" {
        return [
            "▁HE Y ▁F ON O",
            "▁HE Y ▁PH ON O",
            "▁HE Y ▁PH ONE ▁O",
            "▁HE Y ▁F UN O",
            // The Russian-accented pronunciation captured from the actual
            // microphone is consistently decoded acoustically as "SHE PHONO".
            "▁SHE ▁PH ON O",
            "▁SHE ▁F ON O",
            // Detecting the distinctive product-name tail makes a single
            // spoken "hey fono" sufficient even when HEY is heard as SHE.
            "▁PH ON O",
            "▁F ON O",
        ]
        .join("\n")
            + "\n";
    }

    // The GigaSpeech BPE vocabulary contains OKAY as a complete word piece
    // and FUN as `▁F UN`. Splitting either word character-by-character makes
    // the keyword graph impossible to reach, so the commonly used Fono phrase
    // must be represented with its actual model tokens.
    if normalized == "OKAY FUN" {
        return "▁OKAY ▁F UN\n".into();
    }

    // Fallback: naive character-level tokenization. This will rarely work for
    // arbitrary phrases, but keeps the API from failing silently. A real
    // implementation should tokenize with the model's BPE vocabulary at
    // runtime (see `text2token` in sherpa-onnx).
    let mut out: Vec<String> = Vec::new();
    for (i, word) in normalized.split_whitespace().enumerate() {
        if i > 0 {
            out.push("▁".into());
        } else {
            out.push("▁".into());
        }
        for ch in word.chars() {
            out.push(ch.to_string());
        }
    }
    out.join(" ") + "\n"
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
    diag: Option<&DiagnosticsHandle>,
) {
    if let Some(d) = diag {
        diag::record_event(d, event_name(&event));
    }
    if let Some(cb) = callback.lock().as_ref() {
        cb(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{map_sensitivity, phrase_to_tokens};

    #[test]
    fn default_phrase_uses_model_bpe_pronunciation_variants() {
        let expected = concat!(
            "▁HE Y ▁F ON O\n",
            "▁HE Y ▁PH ON O\n",
            "▁HE Y ▁PH ONE ▁O\n",
            "▁HE Y ▁F UN O\n",
            "▁SHE ▁PH ON O\n",
            "▁SHE ▁F ON O\n",
            "▁PH ON O\n",
            "▁F ON O\n",
        );
        assert_eq!(phrase_to_tokens("hey fono"), expected);
        assert_eq!(phrase_to_tokens("  HEY FONO  "), expected);
    }

    #[test]
    fn okay_fun_uses_the_model_bpe_tokens() {
        assert_eq!(phrase_to_tokens("okay fun"), "▁OKAY ▁F UN\n");
    }

    #[test]
    fn sensitivity_is_clamped_before_mapping() {
        assert_eq!(map_sensitivity(-1.0), 0.5);
        assert_eq!(map_sensitivity(0.0), 0.5);
        assert_eq!(map_sensitivity(1.0), 4.0);
        assert_eq!(map_sensitivity(2.0), 4.0);
    }
}
