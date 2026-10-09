//! Checkpointed API inference. No desktop events, dictation history, or insertion.
use crate::application::live_agreement::LiveAgreement;
use crate::error::{AppError, AppResult};
use crate::stt::WindowTranscript;
use crate::types::Transcript;
use fono_core::OperationCancellation;

const WINDOW_SAMPLES: usize = 20 * 16_000;
const OVERLAP_SAMPLES: usize = 2 * 16_000;
const EDGE_HOLD_SAMPLES: u64 = 16_000;

/// `preempted` describes scheduler cancellation, never user/job cancellation.
pub(super) fn transcribe_windows(
    samples: &[i16],
    cancellation: &OperationCancellation,
    mut window: impl FnMut(&[i16], u64, Option<&str>) -> (AppResult<WindowTranscript>, bool),
) -> AppResult<Transcript> {
    let mut start = 0;
    let mut agreement = LiveAgreement::default();
    let mut inference_secs = 0.0;
    let mut detected_language = None;
    let device = loop {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("service job cancelled".into()));
        }
        let end = (start + WINDOW_SAMPLES).min(samples.len());
        // Text from audio still present in the overlap can make Whisper omit
        // words and attach the remaining words to old timestamps.
        let (result, preempted) = window(&samples[start..end], start as u64, None);
        let mut hypothesis = match result {
            Err(AppError::Cancelled(_)) if preempted && !cancellation.is_cancelled() => continue,
            result => result?,
        };
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "service job cancelled after inference".into(),
            ));
        }
        inference_secs += hypothesis.transcribe_secs;
        if detected_language.is_none() {
            detected_language = hypothesis.detected_language.clone();
        }
        let device = Some(
            match hypothesis.backend {
                fono_stt_protocol::BackendKind::Cpu => "CPU",
                fono_stt_protocol::BackendKind::Cuda => "CUDA",
                fono_stt_protocol::BackendKind::Vulkan => "Vulkan",
            }
            .to_string(),
        );
        let final_window = end == samples.len();
        if !final_window {
            // A word touching the fresh edge is decoded again with audio overlap.
            hypothesis
                .words
                .retain(|word| word.end_sample <= (end as u64).saturating_sub(EDGE_HOLD_SAMPLES));
        }
        agreement.accept(&hypothesis, end as u64, true);
        if final_window {
            break device;
        }
        start = end.saturating_sub(OVERLAP_SAMPLES);
    };
    Ok(Transcript {
        text: agreement.committed,
        detected_language,
        transcribe_secs: Some(inference_secs),
        audio_secs: Some(samples.len() as f32 / 16_000.0),
        device,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::TimedSegment;
    fn hypothesis(start: u64, text: &str) -> WindowTranscript {
        WindowTranscript {
            text: text.into(),
            segments: vec![],
            words: vec![TimedSegment {
                text: text.into(),
                start_sample: start,
                end_sample: start + 1000,
            }],
            detected_language: Some("ru".into()),
            transcribe_secs: 0.1,
            audio_secs: 1.0,
            backend: fono_stt_protocol::BackendKind::Cpu,
        }
    }
    #[test]
    fn scheduler_preemption_retries_identical_checkpoint_without_cancelling_job() {
        let cancellation = OperationCancellation::default();
        let mut offsets = Vec::new();
        let result = transcribe_windows(&vec![0; 1000], &cancellation, |_, start, _| {
            offsets.push(start);
            if offsets.len() <= 20 {
                (Err(AppError::Cancelled("preempted".into())), true)
            } else {
                (Ok(hypothesis(start, "Текст")), false)
            }
        })
        .unwrap();
        assert_eq!(offsets, vec![0; 21]);
        assert_eq!(result.text, "Текст");
        assert!(!cancellation.is_cancelled());
    }
    #[test]
    fn job_cancellation_stops_retries_and_never_publishes_late_result() {
        let cancellation = OperationCancellation::default();
        let result = transcribe_windows(&[0; 1000], &cancellation, |_, start, _| {
            cancellation.cancel();
            (Ok(hypothesis(start, "Поздний текст")), false)
        });
        assert!(matches!(result, Err(AppError::Cancelled(_))));
    }
    #[test]
    fn windows_are_bounded_and_whole_job_duration_is_preserved() {
        let mut lengths = Vec::new();
        let result = transcribe_windows(
            &vec![0; 45 * 16000],
            &OperationCancellation::default(),
            |samples, start, _| {
                lengths.push(samples.len());
                (Ok(hypothesis(start, "да")), false)
            },
        )
        .unwrap();
        assert_eq!(lengths.len(), 3);
        assert!(lengths.iter().all(|length| *length <= WINDOW_SAMPLES));
        assert_eq!(result.audio_secs, Some(45.0));
        assert_eq!(result.text, "да да да");
    }
}
