//! Streaming neural VAD. Every 512-sample window is inspected, including
//! earlier windows in a packet that finishes in silence.
use crate::{WakeWordError, WakeWordResult};
use std::{collections::VecDeque, path::Path};
mod recording;

const WINDOW_SAMPLES: usize = 512;
// Five windows are the earliest possible confirmation with min_speech=0.12 s.
// This excludes only buffers too short for the configured neural detector.
pub const MIN_RECORDING_SAMPLES: usize = WINDOW_SAMPLES * 5;

#[derive(Debug, Clone, Copy)]
pub struct SpeechVadDecision {
    pub current_speech: bool,
    pub has_speech: bool,
    pub last_speech_sample: Option<u64>,
    pub processed_samples: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecordedSpeechDecision {
    pub has_speech: bool,
    pub first_speech_sample: Option<u64>,
    pub last_speech_sample: Option<u64>,
    pub inspected_samples: u64,
}

pub struct StreamingSpeechVad {
    detector: sherpa_onnx::VoiceActivityDetector,
    pending: VecDeque<f32>,
    processed: u64,
    last_speech: Option<u64>,
    has_speech: bool,
    recorded: RecordedSpeechDecision,
}
impl StreamingSpeechVad {
    pub fn new(model_path: &Path) -> WakeWordResult<Self> {
        if !model_path.is_file() {
            return Err(WakeWordError::ModelNotFound(model_path.to_owned()));
        }
        let config = sherpa_onnx::VadModelConfig {
            silero_vad: sherpa_onnx::SileroVadModelConfig {
                model: Some(model_path.to_string_lossy().into_owned()),
                threshold: 0.5,
                min_silence_duration: 0.35,
                min_speech_duration: 0.12,
                window_size: 512,
                max_speech_duration: 30.0,
            },
            sample_rate: 16000,
            num_threads: 1,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        let detector = sherpa_onnx::VoiceActivityDetector::create(&config, 30.0)
            .ok_or_else(|| WakeWordError::ModelLoad("could not initialize Silero VAD".into()))?;
        Ok(Self {
            detector,
            pending: VecDeque::new(),
            processed: 0,
            last_speech: None,
            has_speech: false,
            recorded: RecordedSpeechDecision::default(),
        })
    }
    pub fn accept(&mut self, samples: &[i16]) -> SpeechVadDecision {
        let mut input = samples.iter();
        let mut window = [0.0f32; WINDOW_SAMPLES];
        loop {
            self.pending.extend(
                input
                    .by_ref()
                    .take(WINDOW_SAMPLES - self.pending.len())
                    .map(|&sample| sample as f32 / 32768.0),
            );
            if self.pending.len() < WINDOW_SAMPLES {
                break;
            }
            for sample in &mut window {
                *sample = self.pending.pop_front().unwrap();
            }
            self.detector.accept_waveform(&window);
            self.processed += WINDOW_SAMPLES as u64;
            if self.detector.detected() {
                self.has_speech = true;
                self.last_speech = Some(self.processed);
            }
            self.drain_segments();
        }
        SpeechVadDecision {
            current_speech: self.detector.detected(),
            has_speech: self.has_speech,
            last_speech_sample: self.last_speech,
            processed_samples: self.processed,
        }
    }

    /// Inspect a completed capture, retaining the streaming detector's low
    /// speech-duration limit while requiring distributed acoustic evidence.
    /// No recognised text or microphone access is needed. Cancellation is
    /// checked between 32 ms windows; pending PCM never exceeds one window.
    pub fn analyze_recording(
        &mut self,
        samples: &[i16],
        cancelled: impl Fn() -> bool,
    ) -> Option<RecordedSpeechDecision> {
        self.reset();
        if cancelled() {
            return None;
        }
        if samples.len() < MIN_RECORDING_SAMPLES {
            return Some(RecordedSpeechDecision::default());
        }
        for chunk in samples.chunks(WINDOW_SAMPLES) {
            if cancelled() {
                return None;
            }
            self.accept(chunk);
        }
        if !self.pending.is_empty() {
            // Evaluate the captured tail as well, without adding PCM to STT.
            let padding = vec![0; WINDOW_SAMPLES - self.pending.len()];
            self.accept(&padding);
        }
        if cancelled() {
            return None;
        }
        self.detector.flush();
        self.drain_segments();
        self.recorded.inspected_samples = samples.len() as u64;
        self.recorded.last_speech_sample = self
            .recorded
            .last_speech_sample
            .map(|sample| sample.min(samples.len() as u64));
        Some(self.recorded)
    }

    fn drain_segments(&mut self) {
        while !self.detector.is_empty() {
            if let Some(segment) = self.detector.front() {
                let start = segment.start().max(0) as u64;
                let end = start + segment.n().max(0) as u64;
                self.last_speech = Some(self.last_speech.unwrap_or_default().max(end));
                self.has_speech = true;
                if recording::sustained_signal(segment.samples()) {
                    self.recorded.has_speech = true;
                    self.recorded.first_speech_sample.get_or_insert(start);
                    self.recorded.last_speech_sample = Some(end);
                }
            }
            self.detector.pop();
        }
    }
    pub fn reset(&mut self) {
        self.detector.reset();
        self.pending.clear();
        self.processed = 0;
        self.last_speech = None;
        self.has_speech = false;
        self.recorded = RecordedSpeechDecision::default();
    }
}

#[cfg(test)]
mod tests;
