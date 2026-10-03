//! Streaming neural VAD. Every 512-sample window is inspected, including
//! earlier windows in a packet that finishes in silence.
use crate::{WakeWordError, WakeWordResult};
use std::{collections::VecDeque, path::Path};

#[derive(Debug, Clone, Copy)]
pub struct SpeechVadDecision {
    pub current_speech: bool,
    pub has_speech: bool,
    pub last_speech_sample: Option<u64>,
    pub processed_samples: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires FONO_VAD_MODEL and FONO_VAD_SPEECH_WAV (external speech fixture)"]
    fn a_packet_ending_in_silence_preserves_its_earlier_speech() {
        let model = std::env::var("FONO_VAD_MODEL").expect("set Silero model path");
        let speech = std::env::var("FONO_VAD_SPEECH_WAV").expect("set 16 kHz speech WAV path");
        let wav = sherpa_onnx::Wave::read(&speech).expect("read speech WAV");
        assert_eq!(wav.sample_rate(), 16_000);
        let mut samples: Vec<i16> = wav
            .samples()
            .iter()
            .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        samples.extend(std::iter::repeat(0).take(32_000));
        let mut vad = StreamingSpeechVad::new(Path::new(&model)).unwrap();
        let whole = vad.accept(&samples);
        assert!(
            whole.has_speech,
            "must inspect speech before trailing silence"
        );
        assert!(
            !whole.current_speech,
            "two seconds of silence must finish speech"
        );
        assert!(whole.last_speech_sample.is_some());
        assert!(whole.last_speech_sample.unwrap() < whole.processed_samples - 16_000);

        vad.reset();
        let mut split = vad.accept(&[]);
        for chunk in samples.chunks(137) {
            split = vad.accept(chunk);
        }
        assert_eq!(whole.has_speech, split.has_speech);
        assert_eq!(whole.current_speech, split.current_speech);
        assert_eq!(whole.last_speech_sample, split.last_speech_sample);
        assert_eq!(whole.processed_samples, split.processed_samples);
        vad.reset();
        let silence = vad.accept(&[0; 32_000]);
        assert!(!silence.has_speech);
        assert_eq!(silence.last_speech_sample, None);
    }
}

pub struct StreamingSpeechVad {
    detector: sherpa_onnx::VoiceActivityDetector,
    pending: VecDeque<f32>,
    processed: u64,
    last_speech: Option<u64>,
    has_speech: bool,
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
        })
    }
    pub fn accept(&mut self, samples: &[i16]) -> SpeechVadDecision {
        self.pending
            .extend(samples.iter().map(|&s| s as f32 / 32768.0));
        let mut window = [0.0f32; 512];
        while self.pending.len() >= 512 {
            for sample in &mut window {
                *sample = self.pending.pop_front().unwrap();
            }
            self.detector.accept_waveform(&window);
            self.processed += 512;
            if self.detector.detected() {
                self.has_speech = true;
                self.last_speech = Some(self.processed);
            }
            while !self.detector.is_empty() {
                if let Some(segment) = self.detector.front() {
                    let end = segment.start().max(0) as u64 + segment.n().max(0) as u64;
                    self.last_speech = Some(self.last_speech.unwrap_or_default().max(end));
                    self.has_speech = true;
                }
                self.detector.pop();
            }
        }
        SpeechVadDecision {
            current_speech: self.detector.detected(),
            has_speech: self.has_speech,
            last_speech_sample: self.last_speech,
            processed_samples: self.processed,
        }
    }
    pub fn reset(&mut self) {
        self.detector.reset();
        self.pending.clear();
        self.processed = 0;
        self.last_speech = None;
        self.has_speech = false;
    }
}
