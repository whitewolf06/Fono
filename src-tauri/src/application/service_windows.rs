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
    progress: &(dyn Fn(f32) + Sync),
) -> AppResult<(
    Transcript,
    Vec<crate::application::transcription_contract::SubtitleSegment>,
)> {
    let mut start = 0;
    let mut agreement = LiveAgreement::default();
    let mut words = Vec::new();
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
        words.extend(agreement.accept_timed(&hypothesis, end as u64, true).1);
        progress(end as f32 / samples.len().max(1) as f32);
        if final_window {
            break device;
        }
        start = end.saturating_sub(OVERLAP_SAMPLES);
    };
    Ok((
        Transcript {
            text: agreement.committed,
            detected_language,
            transcribe_secs: Some(inference_secs),
            audio_secs: Some(samples.len() as f32 / 16_000.0),
            device,
        },
        crate::application::transcript_export::cues(&words, samples.len() as u64),
    ))
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
    fn thirty_minute_file_keeps_global_offsets_across_preemption_and_exports_last_cue() {
        let samples = vec![0; 1800 * 16_000];
        let mut calls = Vec::new();
        let progress = std::sync::Mutex::new(Vec::new());
        let (transcript, cues) = transcribe_windows(
            &samples,
            &OperationCancellation::default(),
            |_, start, _| {
                calls.push(start);
                if calls.len() == 2 {
                    return (Err(AppError::Cancelled("dictation preempted".into())), true);
                }
                (Ok(hypothesis(start, &format!("Сегмент {start}."))), false)
            },
            &|fraction| progress.lock().unwrap().push(fraction),
        )
        .unwrap();
        assert_eq!(calls[1], calls[2]);
        assert_eq!(transcript.audio_secs, Some(1800.0));
        assert!(cues.len() > 90);
        assert!(cues
            .windows(2)
            .all(|pair| pair[0].end_ms <= pair[1].start_ms));
        assert!(cues.last().unwrap().start_ms > 1_780_000);
        assert_eq!(progress.lock().unwrap().last(), Some(&1.0));
        let mut result =
            crate::application::transcription_contract::TranscriptionResult::from_transcript(
                transcript,
                "base".into(),
            );
        result.segments = cues;
        assert!(crate::application::transcript_export::render(
            &result,
            crate::application::transcript_export::ExportFormat::Srt
        )
        .unwrap()
        .contains("00:29:"));
    }
    #[test]
    fn scheduler_preemption_retries_identical_checkpoint_without_cancelling_job() {
        let cancellation = OperationCancellation::default();
        let mut offsets = Vec::new();
        let result = transcribe_windows(
            &vec![0; 1000],
            &cancellation,
            |_, start, _| {
                offsets.push(start);
                if offsets.len() <= 20 {
                    (Err(AppError::Cancelled("preempted".into())), true)
                } else {
                    (Ok(hypothesis(start, "Текст")), false)
                }
            },
            &|_| {},
        )
        .unwrap()
        .0;
        assert_eq!(offsets, vec![0; 21]);
        assert_eq!(result.text, "Текст");
        assert!(!cancellation.is_cancelled());
    }
    #[test]
    fn job_cancellation_stops_retries_and_never_publishes_late_result() {
        let cancellation = OperationCancellation::default();
        let result = transcribe_windows(
            &[0; 1000],
            &cancellation,
            |_, start, _| {
                cancellation.cancel();
                (Ok(hypothesis(start, "Поздний текст")), false)
            },
            &|_| {},
        );
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
            &|_| {},
        )
        .unwrap()
        .0;
        assert_eq!(lengths.len(), 3);
        assert!(lengths.iter().all(|length| *length <= WINDOW_SAMPLES));
        assert_eq!(result.audio_secs, Some(45.0));
        assert_eq!(result.text, "да да да");
    }
}
