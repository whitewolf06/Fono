//! One segmentation policy for live capture and reproducible WAV replay.
use super::sherpa_detector::{Detection, SherpaDetector};
use std::collections::VecDeque;

pub(crate) trait Decoder {
    fn accept(&mut self, samples: &[f32], clock: u64) -> Option<Detection>;
    fn finish(&mut self, clock: u64) -> Option<Detection>;
    fn is_endpoint(&self) -> bool;
    fn reset(&mut self);
}
impl Decoder for SherpaDetector {
    fn accept(&mut self, samples: &[f32], clock: u64) -> Option<Detection> {
        self.accept(samples, clock)
    }
    fn finish(&mut self, clock: u64) -> Option<Detection> {
        self.finish(clock)
    }
    fn is_endpoint(&self) -> bool {
        self.is_endpoint()
    }
    fn reset(&mut self) {
        self.reset();
    }
}
struct BufferedAudio {
    samples: Vec<f32>,
    start: u64,
    end: u64,
}
pub(crate) struct SessionDetection {
    pub detection: Detection,
    pub phrase_end_sample: u64,
}
pub(crate) struct DecoderSession<D: Decoder> {
    decoder: D,
    rate: u64,
    origin: Option<u64>,
    retained: VecDeque<BufferedAudio>,
    retained_samples: usize,
}
impl<D: Decoder> DecoderSession<D> {
    pub(crate) fn new(decoder: D, sample_rate: u32) -> Self {
        Self {
            decoder,
            rate: sample_rate as u64,
            origin: None,
            retained: VecDeque::new(),
            retained_samples: 0,
        }
    }
    pub(crate) fn reset(&mut self) {
        self.decoder.reset();
        self.origin = None;
        self.retained.clear();
        self.retained_samples = 0;
    }
    pub(crate) fn accept(&mut self, samples: &[f32], end: u64) -> Option<SessionDetection> {
        let origin = *self
            .origin
            .get_or_insert(end.saturating_sub(samples.len() as u64));
        self.retained_samples += samples.len();
        self.retained.push_back(BufferedAudio {
            samples: samples.to_vec(),
            start: end.saturating_sub(samples.len() as u64),
            end,
        });
        while self.retained_samples > self.rate as usize * 2 && self.retained.len() > 1 {
            self.retained_samples -= self.retained.pop_front().unwrap().samples.len();
        }
        if let Some(detection) = self.decoder.accept(samples, end - origin) {
            return Some(self.complete(detection, origin, end));
        }
        // A maximum-duration endpoint must rotate with overlap too. Checking
        // endpoint first would lose words crossing that exact boundary.
        if end - origin >= self.rate * 20 {
            self.decoder.reset();
            let first = self.retained.front().map_or(end, |chunk| chunk.start);
            self.origin = Some(first);
            for chunk in &self.retained {
                if let Some(detection) = self.decoder.accept(&chunk.samples, chunk.end - first) {
                    return Some(self.complete(detection, first, end));
                }
            }
        } else if self.decoder.is_endpoint() {
            self.reset();
        }
        None
    }
    pub(crate) fn finish(&mut self, end: u64) -> Option<SessionDetection> {
        let origin = self.origin?;
        let detection = self.decoder.finish(end - origin)?;
        Some(self.complete(detection, origin, end))
    }
    fn complete(&mut self, detection: Detection, origin: u64, end: u64) -> SessionDetection {
        let phrase_end_sample =
            origin + (detection.phrase_end_seconds.max(0.0) as f64 * self.rate as f64) as u64;
        self.reset();
        SessionDetection {
            detection,
            phrase_end_sample: phrase_end_sample.min(end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct EdgeDecoder {
        first_word: bool,
        clock: u64,
    }
    impl Decoder for EdgeDecoder {
        fn accept(&mut self, samples: &[f32], clock: u64) -> Option<Detection> {
            self.clock = clock;
            self.first_word |= samples.contains(&0.4);
            (self.first_word && samples.contains(&0.6)).then(|| Detection {
                phrase: "wake word".into(),
                json: String::new(),
                phrase_end_seconds: clock as f32 / 10.0,
            })
        }
        fn finish(&mut self, _clock: u64) -> Option<Detection> {
            None
        }
        fn is_endpoint(&self) -> bool {
            self.clock >= 200
        }
        fn reset(&mut self) {
            *self = Self::default();
        }
    }
    #[test]
    fn maximum_duration_endpoint_preserves_a_phrase_across_rotation() {
        let mut session = DecoderSession::new(EdgeDecoder::default(), 10);
        for second in 1..=20 {
            let samples = [if second == 19 { 0.4 } else { 0.0 }; 10];
            assert!(session.accept(&samples, second * 10).is_none());
        }
        let result = session.accept(&[0.6; 10], 210).unwrap();
        assert_eq!(result.detection.phrase, "wake word");
        assert_eq!(result.phrase_end_sample, 210);
    }
    #[test]
    fn pause_reset_discards_a_previous_partial_phrase() {
        let mut session = DecoderSession::new(EdgeDecoder::default(), 10);
        assert!(session.accept(&[0.4; 10], 10).is_none());
        session.reset();
        assert!(session.accept(&[0.6; 10], 20).is_none());
    }
}
