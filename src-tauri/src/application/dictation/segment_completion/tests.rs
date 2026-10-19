use super::*;
use crate::pipeline::capture_segments::CaptureSegmentResult;

fn result(text: &str) -> WindowTranscript {
    WindowTranscript {
        text: text.into(),
        segments: vec![],
        words: vec![],
        detected_language: Some("ru".into()),
        transcribe_secs: 0.25,
        audio_secs: 4.0 / 16_000.0,
        backend: BackendKind::Cpu,
    }
}

fn prefix() -> CaptureSegmentBatch {
    CaptureSegmentBatch {
        operation_id: 7,
        profile: CaptureSegmentProfile::from(&Settings::default()),
        results: vec![CaptureSegmentResult {
            operation_id: 7,
            index: 0,
            range: SpeechSegment {
                start: 0,
                end: 4,
                forced: false,
            },
            transcript: Some(result("да")),
        }],
        processed_until: 4,
    }
}

fn tail() -> [SpeechSegment; 2] {
    [
        SpeechSegment {
            start: 4,
            end: 8,
            forced: false,
        },
        SpeechSegment {
            start: 8,
            end: 12,
            forced: false,
        },
    ]
}

#[test]
fn a_ready_prefix_is_reused_and_only_disjoint_tail_ranges_are_recognized_with_context() {
    let samples = (0..12).collect::<Vec<i16>>();
    let mut calls = Vec::new();
    let transcript = complete(
        7,
        &Settings::default(),
        &samples,
        prefix(),
        &tail(),
        &OperationCancellation::default(),
        |window, start, context| {
            calls.push((window.to_vec(), start, context.map(str::to_owned)));
            Ok(Some(result(if start == 4 {
                "да"
            } else {
                ", дальше"
            })))
        },
    )
    .unwrap();
    assert_eq!(
        calls,
        vec![
            (vec![4, 5, 6, 7], 4, Some("да".into())),
            (vec![8, 9, 10, 11], 8, Some("да".into()))
        ]
    );
    assert_eq!(transcript.text, "да да, дальше");
    assert_eq!(transcript.transcribe_secs, Some(0.75));
    assert_eq!(transcript.audio_secs, Some(12.0 / 16_000.0));
    assert_eq!(transcript.device.as_deref(), Some("CPU"));
}

#[test]
fn a_complete_prefix_never_enters_stt_and_silence_does_not_invent_a_device() {
    let ready = complete(
        7,
        &Settings::default(),
        &[0; 4],
        prefix(),
        &[],
        &OperationCancellation::default(),
        |_, _, _| panic!("ready PCM must not be decoded again"),
    )
    .unwrap();
    assert_eq!(ready.text, "да");
    let mut silent = prefix();
    silent.results[0].transcript = None;
    let silent = complete(
        7,
        &Settings::default(),
        &[0; 8],
        silent,
        &tail()[..1],
        &OperationCancellation::default(),
        |_, _, context| {
            assert_eq!(context, None);
            Ok(None)
        },
    )
    .unwrap();
    assert!(silent.text.is_empty());
    assert_eq!(silent.device, None);
    assert_eq!(silent.transcribe_secs, None);
}

#[test]
fn stale_identity_settings_and_broken_checkpoints_are_rejected_before_tail_inference() {
    let mut cases = Vec::new();
    let mut batch = prefix();
    batch.operation_id = 8;
    cases.push(batch);
    let mut batch = prefix();
    batch.results[0].operation_id = 8;
    cases.push(batch);
    let mut batch = prefix();
    batch.results[0].index = 1;
    cases.push(batch);
    let mut batch = prefix();
    batch.results[0].range.start = 1;
    cases.push(batch);
    let mut batch = prefix();
    batch.processed_until = 5;
    cases.push(batch);
    let mut batch = prefix();
    batch.results[0]
        .transcript
        .as_mut()
        .unwrap()
        .transcribe_secs = f32::NAN;
    cases.push(batch);
    for batch in cases {
        assert!(complete(
            7,
            &Settings::default(),
            &[0; 12],
            batch,
            &tail(),
            &OperationCancellation::default(),
            |_, _, _| panic!("invalid prefix reached STT")
        )
        .is_err());
    }
    let changed = Settings {
        language: "en".into(),
        ..Settings::default()
    };
    assert!(validate_prefix(7, &changed, 12, &prefix()).is_err());
    let changed = Settings {
        whisper_model_path: Some("other-model.bin".into()),
        ..Settings::default()
    };
    assert!(validate_prefix(7, &changed, 12, &prefix()).is_err());
    assert!(complete(
        7,
        &Settings::default(),
        &[0; 12],
        prefix(),
        &tail()[..1],
        &OperationCancellation::default(),
        |_, _, _| panic!("incomplete tail reached STT")
    )
    .is_err());
}

#[test]
fn cancellation_discards_even_a_successful_late_tail_and_backend_changes_fail_closed() {
    let cancellation = OperationCancellation::default();
    let value = complete(
        7,
        &Settings::default(),
        &[0; 8],
        prefix(),
        &tail()[..1],
        &cancellation,
        |_, _, _| {
            cancellation.cancel();
            Ok(Some(result("поздно")))
        },
    );
    assert!(matches!(value, Err(AppError::Cancelled(_))));
    let value = complete(
        7,
        &Settings::default(),
        &[0; 4],
        prefix(),
        &[],
        &cancellation,
        |_, _, _| panic!("cancelled work reached STT"),
    );
    assert!(matches!(value, Err(AppError::Cancelled(_))));
    let value = complete(
        7,
        &Settings::default(),
        &[0; 8],
        prefix(),
        &tail()[..1],
        &OperationCancellation::default(),
        |_, _, _| {
            let mut result = result("новый backend");
            result.backend = BackendKind::Vulkan;
            Ok(Some(result))
        },
    );
    assert!(matches!(value, Err(AppError::Stt(_))));
}

#[test]
fn the_tail_uses_capture_pause_policy_and_preserves_every_sample_after_the_checkpoint() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut phrase = crate::application::audio_ingest::decode_file(
        &root.join("tests/fixtures/audio/jfk-short-speech-16k-mono.wav"),
        Default::default(),
    )
    .unwrap()
    .pcm_samples;
    phrase.extend(vec![0; 16_000]);
    let mut samples = vec![7; 137];
    samples.extend(phrase.repeat(3));
    samples.extend([1; 131]);
    let cancellation = OperationCancellation::default();
    let ranges = tail_ranges(
        &samples,
        137,
        &root.join("resources/vad/silero_vad.onnx"),
        &cancellation,
    )
    .unwrap();
    assert!(ranges.len() >= 3);
    let mut expected = 137;
    for range in ranges {
        assert_eq!(range.start, expected);
        assert!(range.end > range.start && range.end - range.start <= DEFAULT_MAX_SEGMENT_SAMPLES);
        expected = range.end;
    }
    assert_eq!(expected, samples.len());
    cancellation.cancel();
    assert!(matches!(
        tail_ranges(
            &samples,
            137,
            &root.join("resources/vad/silero_vad.onnx"),
            &cancellation
        ),
        Err(AppError::Cancelled(_))
    ));
}

#[tokio::test]
async fn duplicate_stop_waiters_receive_one_segmented_result_without_repeating_tail_calls() {
    use crate::pipeline::completion::{wait, CompletionGate, FinishClaim};
    let gate = CompletionGate::default();
    let FinishClaim::Owner(ticket) = gate.claim(7).unwrap() else {
        panic!("first Stop must own completion");
    };
    let FinishClaim::Waiting(waiter) = gate.claim(7).unwrap() else {
        panic!("second Stop must wait");
    };
    let mut calls = 0;
    let transcript = complete(
        7,
        &Settings::default(),
        &[0; 8],
        prefix(),
        &tail()[..1],
        &OperationCancellation::default(),
        |_, _, _| {
            calls += 1;
            Ok(Some(result("хвост")))
        },
    )
    .unwrap();
    ticket.complete(&Ok(
        crate::application::dictation::DictationOutcome::Completed(transcript),
    ));
    for result in [wait(waiter).await.unwrap(), {
        let FinishClaim::Waiting(waiter) = gate.claim(0).unwrap() else {
            panic!();
        };
        wait(waiter).await.unwrap()
    }] {
        assert!(
            matches!(result, crate::application::dictation::DictationOutcome::Completed(text) if text.text == "да хвост")
        );
    }
    assert_eq!(calls, 1);
}
