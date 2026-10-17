//! STT preparation projects the same Silero decision that admitted this capture.
//! No independent energy threshold can discard a quiet word or recorded tail.

pub use fono_audio::SpeechBounds as VadResult;

pub fn prepare_recording_with_result(
    samples: &[i16],
    decision: &fono_audio::RecordedSpeechDecision,
) -> (Vec<i16>, VadResult) {
    let bounds = fono_audio::recorded_speech_bounds(samples.len(), decision);
    (samples[bounds.start_sample..].to_vec(), bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_preserves_every_sample_including_quiet_onset_and_tail() {
        // All-zero values deliberately prove that RMS is never consulted.
        let samples = vec![0; 60_000];
        let decision = fono_audio::RecordedSpeechDecision {
            has_speech: true,
            first_speech_sample: Some(40_000),
            last_speech_sample: Some(40_500),
            inspected_samples: samples.len() as u64,
        };
        let (trimmed, result) = prepare_recording_with_result(&samples, &decision);
        assert!(result.has_speech);
        assert_eq!(result.start_sample, 0);
        assert_eq!(result.speech_end_sample, 40_500);
        assert_eq!(trimmed, samples);
        assert_eq!(trimmed.len(), 60_000);
    }
}
