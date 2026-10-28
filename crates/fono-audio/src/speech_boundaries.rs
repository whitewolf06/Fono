//! Shared projection of confirmed neural segments into STT buffer boundaries.

#[derive(Debug, Clone, Copy, Default)]
pub struct RecordedSpeechDecision {
    pub timing: Option<crate::SpeechTimingSamples>,
    pub has_speech: bool,
    pub first_speech_sample: Option<u64>,
    pub last_speech_sample: Option<u64>,
    pub inspected_samples: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SpeechBounds {
    pub start_sample: usize,
    pub end_sample: usize,
    pub speech_end_sample: usize,
    pub has_speech: bool,
    pub trimmed_samples: usize,
}

/// The same confirmed neural segment admits speech and supplies diagnostics.
/// Keep the complete PCM for STT: 150 ms and 2 s leading-trim candidates both
/// regressed fixed-corpus WER. No second energy gate or unqualified trimming
/// optimisation may discard the onset of a quiet phrase or its recorded tail.
pub fn recorded_speech_bounds(length: usize, decision: &RecordedSpeechDecision) -> SpeechBounds {
    let bounds = decision
        .first_speech_sample
        .zip(decision.last_speech_sample);
    let speech_end_sample = match bounds {
        Some((first, last))
            if decision.has_speech
                && first < last
                && first < length as u64
                && decision.inspected_samples == length as u64 =>
        {
            last.min(length as u64) as usize
        }
        _ => {
            return SpeechBounds {
                start_sample: 0,
                end_sample: length,
                speech_end_sample: 0,
                has_speech: false,
                trimmed_samples: 0,
            };
        }
    };
    SpeechBounds {
        start_sample: 0,
        end_sample: length,
        speech_end_sample,
        has_speech: true,
        trimmed_samples: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neural_boundaries_keep_the_entire_onset_and_recorded_tail_for_stt() {
        let decision = RecordedSpeechDecision {
            has_speech: true,
            first_speech_sample: Some(40_000),
            last_speech_sample: Some(40_500),
            inspected_samples: 60_000,
            timing: None,
        };
        let bounds = recorded_speech_bounds(60_000, &decision);
        assert_eq!(bounds.start_sample, 0);
        assert_eq!(bounds.end_sample, 60_000);
        assert_eq!(bounds.speech_end_sample, 40_500);
        assert_eq!(bounds.trimmed_samples, 0);
    }

    #[test]
    fn silence_cancelled_prefix_and_inconsistent_segments_never_trim_audio() {
        for decision in [
            RecordedSpeechDecision::default(),
            RecordedSpeechDecision {
                has_speech: true,
                first_speech_sample: Some(5_000),
                last_speech_sample: Some(4_000),
                inspected_samples: 10_000,
                timing: None,
            },
            RecordedSpeechDecision {
                has_speech: true,
                first_speech_sample: Some(5_000),
                last_speech_sample: Some(8_000),
                inspected_samples: 8_000,
                timing: None,
            },
        ] {
            let bounds = recorded_speech_bounds(10_000, &decision);
            assert!(!bounds.has_speech);
            assert_eq!(bounds.start_sample, 0);
            assert_eq!(bounds.end_sample, 10_000);
        }
    }
}
