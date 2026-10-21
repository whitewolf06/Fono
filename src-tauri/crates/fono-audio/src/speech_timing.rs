//! Numeric union of confirmed neural speech segments. No PCM or segment list retained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpeechTimingSamples {
    pub speech: u64,
    pub pause_count: u32,
    pub pauses: u64,
    pub max_pause: u64,
}

#[derive(Default)]
#[cfg(any(feature = "speech-vad", test))]
pub(crate) struct SpeechTimingAccumulator {
    totals: SpeechTimingSamples,
    end: Option<u64>,
}
#[cfg(any(feature = "speech-vad", test))]
impl SpeechTimingAccumulator {
    pub fn push(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let previous = self.end.unwrap_or(start);
        if self.end.is_some() && start > previous {
            let pause = start - previous;
            self.totals.pause_count += 1;
            self.totals.pauses += pause;
            self.totals.max_pause = self.totals.max_pause.max(pause);
        }
        self.totals.speech += end.saturating_sub(start.max(previous));
        self.end = Some(previous.max(end));
    }
    pub fn summary(&self) -> Option<SpeechTimingSamples> {
        (self.totals.speech > 0).then_some(self.totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "speech-vad")]
    #[test]
    fn public_wav_segments_produce_bounded_summary_without_pcm_retention() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let wav =
            std::fs::read(root.join("tests/fixtures/audio/jfk-short-speech-16k-mono.wav")).unwrap();
        let speech = wav[44..]
            .chunks_exact(2)
            .map(|s| i16::from_le_bytes([s[0], s[1]]))
            .collect::<Vec<_>>();
        let mut samples = vec![0; 1600];
        samples.extend(&speech);
        samples.extend(vec![0; 48000]);
        samples.extend(&speech);
        samples.extend(vec![0; 1631]);
        let mut vad =
            crate::StreamingSpeechVad::new(&root.join("resources/vad/silero_vad.onnx")).unwrap();
        let decision = vad.analyze_recording(&samples, || false).unwrap();
        assert!(decision.has_speech);
        let timing = decision.timing.unwrap();
        assert!(timing.speech > 0);
        assert_eq!(timing.pause_count, 1);
        assert_eq!(timing.max_pause, timing.pauses);
        assert!(timing.speech + timing.pauses <= samples.len() as u64);
        assert!(vad.analyze_recording(&samples, || true).is_none());
    }
    #[test]
    fn union_excludes_outer_silence_and_does_not_count_overlap_or_forced_splits_as_pauses() {
        let mut timing = SpeechTimingAccumulator::default();
        assert_eq!(timing.summary(), None);
        for (start, end) in [
            (1600, 4800),
            (4000, 6400),
            (6400, 8000),
            (11200, 12800),
            (19200, 22400),
        ] {
            timing.push(start, end);
        }
        assert_eq!(
            timing.summary(),
            Some(SpeechTimingSamples {
                speech: 11200,
                pause_count: 2,
                pauses: 9600,
                max_pause: 6400
            })
        );
    }
}
