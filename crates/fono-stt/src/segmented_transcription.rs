//! Non-overlapping speech segments are checkpoints: preemption retries only
//! the uncommitted segment; user cancellation discards the complete result.
use fono_audio::speech_segments::SpeechSegment;
use fono_core::OperationCancellation;
use fono_stt_protocol::WindowTranscript;
use fono_types::{CoreError, ErrorCode};

/// Error adaptation keeps a caller's domain error without coupling the algorithm
/// to an application or changing which failures may be retried after preemption.
pub trait SegmentError: From<CoreError> {
    fn is_cancelled(&self) -> bool;
}

impl SegmentError for crate::error::SttError {
    fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }
}

pub fn transcribe_segments<E: SegmentError>(
    samples: &[i16],
    segments: &[SpeechSegment],
    cancellation: &OperationCancellation,
    mut recognize: impl FnMut(&[i16], u64) -> (Result<WindowTranscript, E>, bool),
) -> Result<WindowTranscript, E> {
    let mut expected = 0;
    for segment in segments {
        if segment.start != expected || segment.end <= segment.start || segment.end > samples.len()
        {
            return Err(CoreError::new(
                ErrorCode::InvalidRequest,
                "segments must partition the complete PCM in order",
                false,
            )
            .into());
        }
        expected = segment.end;
    }
    if expected != samples.len() {
        return Err(
            CoreError::new(ErrorCode::InvalidRequest, "segmentation lost PCM", false).into(),
        );
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
                return Err(
                    CoreError::new(ErrorCode::Cancelled, "segmented job cancelled", false).into(),
                );
            }
            let (result, preempted) =
                recognize(&samples[segment.start..segment.end], segment.start as u64);
            if cancellation.is_cancelled() {
                return Err(CoreError::new(
                    ErrorCode::Cancelled,
                    "segmented result cancelled",
                    false,
                )
                .into());
            }
            match result {
                Err(error) if preempted && error.is_cancelled() => continue,
                result => break result?,
            }
        };
        if backend.is_some_and(|backend| backend != result.backend) {
            return Err(CoreError::new(
                ErrorCode::WorkerFailed,
                "recognizer device changed between segments",
                false,
            )
            .into());
        }
        backend = Some(result.backend);
        combined.backend = result.backend;
        append_text(&mut combined.text, &result.text);
        combined.transcribe_secs += result.transcribe_secs;
        if combined.detected_language.is_none() {
            combined.detected_language = result.detected_language;
        }
        combined.segments.extend(result.segments);
        combined.words.extend(result.words);
    }
    Ok(combined)
}

pub fn append_text(combined: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !combined.is_empty() && !text.starts_with(['.', ',', '!', '?', ':', ';']) {
        combined.push(' ');
    }
    combined.push_str(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use fono_stt_protocol::TimedSegment;
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
            transcribe_segments::<AppError>(&[0; 8], &ranges, &cancel, |_, _| panic!(
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

    #[derive(Debug, PartialEq, Eq)]
    enum DomainError {
        Core(CoreError),
        Backend(&'static str),
    }

    impl From<CoreError> for DomainError {
        fn from(error: CoreError) -> Self {
            Self::Core(error)
        }
    }

    impl SegmentError for DomainError {
        fn is_cancelled(&self) -> bool {
            matches!(self, Self::Core(error) if error.code == ErrorCode::Cancelled)
        }
    }

    #[test]
    fn preemption_does_not_retry_or_replace_a_callers_non_cancellation_error() {
        let ranges = [SpeechSegment {
            start: 0,
            end: 4,
            forced: false,
        }];
        let mut calls = 0;
        let error = transcribe_segments(
            &[0; 4],
            &ranges,
            &OperationCancellation::default(),
            |_, _| {
                calls += 1;
                (Err(DomainError::Backend("exact domain failure")), true)
            },
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error, DomainError::Backend("exact domain failure"));
    }

    #[test]
    fn invalid_ranges_keep_the_callers_error_type_without_starting_recognition() {
        let error = transcribe_segments::<DomainError>(
            &[0; 4],
            &[],
            &OperationCancellation::default(),
            |_, _| panic!("invalid partition reached recognition"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            DomainError::Core(CoreError {
                code: ErrorCode::InvalidRequest,
                ..
            })
        ));
    }
}
