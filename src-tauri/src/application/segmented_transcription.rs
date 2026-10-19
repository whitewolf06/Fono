//! Non-overlapping speech segments are checkpoints: preemption retries only
//! the uncommitted segment; user cancellation discards the complete result.
use crate::{
    error::{AppError, AppResult},
    stt::WindowTranscript,
};
use fono_audio::speech_segments::SpeechSegment;
use fono_core::OperationCancellation;

pub fn transcribe_segments(
    samples: &[i16],
    segments: &[SpeechSegment],
    cancellation: &OperationCancellation,
    mut recognize: impl FnMut(&[i16], u64) -> (AppResult<WindowTranscript>, bool),
) -> AppResult<WindowTranscript> {
    let mut expected = 0;
    for segment in segments {
        if segment.start != expected || segment.end <= segment.start || segment.end > samples.len()
        {
            return Err(AppError::Config(
                "segments must partition the complete PCM in order".into(),
            ));
        }
        expected = segment.end;
    }
    if expected != samples.len() {
        return Err(AppError::Config("segmentation lost PCM".into()));
    }
    let mut combined = WindowTranscript {
        text: String::new(),
        segments: Vec::new(),
        words: Vec::new(),
        detected_language: None,
        transcribe_secs: 0.0,
        audio_secs: samples.len() as f32 / 16_000.0,
        backend: fono_stt_protocol::BackendKind::Cpu,
    };
    let mut backend = None;
    for segment in segments {
        let result = loop {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("segmented job cancelled".into()));
            }
            let (result, preempted) =
                recognize(&samples[segment.start..segment.end], segment.start as u64);
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("segmented result cancelled".into()));
            }
            match result {
                Err(AppError::Cancelled(_)) if preempted => continue,
                result => break result?,
            }
        };
        if backend.is_some_and(|backend| backend != result.backend) {
            return Err(AppError::Stt(
                "recognizer device changed between segments".into(),
            ));
        }
        backend = Some(result.backend);
        combined.backend = result.backend;
        let text = result.text.trim();
        if !text.is_empty() {
            if !combined.text.is_empty() && !text.starts_with(['.', ',', '!', '?', ':', ';']) {
                combined.text.push(' ');
            }
            combined.text.push_str(text);
        }
        combined.transcribe_secs += result.transcribe_secs;
        if combined.detected_language.is_none() {
            combined.detected_language = result.detected_language;
        }
        combined.segments.extend(result.segments);
        combined.words.extend(result.words);
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::TimedSegment;
    fn result(start: u64) -> WindowTranscript {
        WindowTranscript {
            text: "yes".into(),
            segments: vec![],
            words: vec![TimedSegment {
                text: "yes".into(),
                start_sample: start,
                end_sample: start + 2,
            }],
            detected_language: None,
            transcribe_secs: 0.1,
            audio_secs: 0.1,
            backend: fono_stt_protocol::BackendKind::Cpu,
        }
    }
    #[test]
    fn preemption_retries_exact_segment_and_repeated_words_are_preserved() {
        let ranges = [
            SpeechSegment {
                start: 0,
                end: 4,
                forced: false,
            },
            SpeechSegment {
                start: 4,
                end: 8,
                forced: false,
            },
        ];
        let mut calls = Vec::new();
        let combined = transcribe_segments(
            &[0; 8],
            &ranges,
            &OperationCancellation::default(),
            |_, start| {
                calls.push(start);
                if calls.len() == 2 {
                    (Err(AppError::Cancelled("preempted".into())), true)
                } else {
                    (Ok(result(start)), false)
                }
            },
        )
        .unwrap();
        assert_eq!(calls, [0, 4, 4]);
        assert_eq!(combined.text, "yes yes");
        assert_eq!(combined.words[1].start_sample, 4);
        assert_eq!(combined.audio_secs, 8.0 / 16_000.0);
    }
    #[test]
    fn cancellation_and_invalid_ranges_never_publish_a_partial_result() {
        let cancel = OperationCancellation::default();
        let ranges = [SpeechSegment {
            start: 0,
            end: 4,
            forced: false,
        }];
        assert!(
            transcribe_segments(&[0; 8], &ranges, &cancel, |_, _| panic!(
                "incomplete partition reached native work"
            ))
            .is_err()
        );
        assert!(matches!(
            transcribe_segments(&[0; 4], &ranges, &cancel, |_, start| {
                cancel.cancel();
                (Ok(result(start)), false)
            }),
            Err(AppError::Cancelled(_))
        ));
    }
}
