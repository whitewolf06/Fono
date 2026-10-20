use super::*;

#[test]
fn prefix_profile_cannot_cross_glossary_changes_but_disabled_entries_stay_private() {
    let mut settings = crate::types::Settings {
        personal_dictionary_enabled: true,
        personal_dictionary_entries: vec![crate::types::PersonalDictionaryEntry {
            written: "Fono".into(),
            spoken: vec!["фоно".into()],
        }],
        ..Default::default()
    };
    let initial = CaptureSegmentProfile::from(&settings);
    settings.personal_dictionary_entries[0].written = "WhiteLife".into();
    assert_ne!(initial, CaptureSegmentProfile::from(&settings));
    settings.personal_dictionary_enabled = false;
    let disabled = CaptureSegmentProfile::from(&settings);
    settings.personal_dictionary_entries[0].written = "PRIVATE".into();
    assert_eq!(disabled, CaptureSegmentProfile::from(&settings));
}
use crate::{
    audio::{AudioRecorder, RecordingWriter},
    pipeline::Pipeline,
};
use fono_core::OperationSource;
use fono_stt_protocol::BackendKind;
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    mpsc,
};

fn vad_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/vad/silero_vad.onnx")
}

fn speech() -> Vec<i16> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio/jfk-short-speech-16k-mono.wav");
    let mut samples = crate::application::audio_ingest::decode_file(&path, Default::default())
        .unwrap()
        .pcm_samples;
    samples.extend(vec![0; 2 * 16_000]);
    samples
}

struct Recorder(Vec<i16>);
impl AudioRecorder<RecordingWriter, AppError> for Recorder {
    fn start(
        &self,
        _: Option<&str>,
        writer: RecordingWriter,
        _: Arc<AtomicBool>,
        _: Arc<AtomicU32>,
        maximum: usize,
    ) -> AppResult<()> {
        assert!(!writer.lock().append(&self.0, maximum));
        Ok(())
    }
    fn stop(&self) -> AppResult<()> {
        Ok(())
    }
    fn shutdown(&self) {}
}

fn pipeline(phrases: usize) -> Pipeline {
    Pipeline::new_with_audio_owner(Box::new(Recorder(speech().repeat(phrases))))
}

fn transcript(samples: usize, text: &str) -> WindowTranscript {
    WindowTranscript {
        text: text.into(),
        segments: Vec::with_capacity(128),
        words: Vec::with_capacity(128),
        detected_language: Some("en".into()),
        transcribe_secs: 0.01,
        audio_secs: samples as f32 / 16_000.0,
        backend: BackendKind::Cpu,
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "capture worker deadline"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn fifteen_second_threshold_and_worker_progress_describe_actual_work() {
    use super::super::pretranscription::{Phase, START_AFTER_SAMPLES};
    let pipeline = pipeline(1);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let length = pipeline.recording_end() as usize;
    assert!(length < START_AFTER_SAMPLES);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut first = true;
    pipeline
        .start_capture_segments_for(
            operation,
            vad_path(),
            START_AFTER_SAMPLES,
            move |samples, _, _, _| {
                if first {
                    first = false;
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                Ok(transcript(samples.len(), "prefix"))
            },
        )
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        pipeline.pretranscription_status_for(operation).phase,
        Phase::Waiting
    );
    assert!(started_rx.try_recv().is_err());
    let scheduler = pipeline.scheduler();
    let permit = scheduler
        .acquire(true, &OperationCancellation::default())
        .unwrap();
    let writer = pipeline.writer.lock().as_ref().unwrap().clone();
    assert!(!writer
        .lock()
        .append(&vec![0; START_AFTER_SAMPLES - length - 1], 5 * 60 * 16_000));
    thread::sleep(Duration::from_millis(40));
    assert_eq!(
        pipeline.pretranscription_status_for(operation).phase,
        Phase::Waiting
    );
    assert!(!writer.lock().append(&[0], 5 * 60 * 16_000));
    wait_until(|| pipeline.pretranscription_status_for(operation).phase == Phase::Queued);
    assert!(started_rx.try_recv().is_err());
    drop(permit);
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let status = pipeline.pretranscription_status_for(operation);
    assert_eq!(status.phase, Phase::Recognizing);
    assert_eq!(status.completed_segments, 0);
    assert_eq!(status.start_after_ms, 15_000);
    release_tx.send(()).unwrap();
    wait_until(|| pipeline.completed_capture_segments_for(operation) == Some(1));
    assert_eq!(
        pipeline.pretranscription_status_for(operation + 1).phase,
        Phase::Inactive
    );
    assert_eq!(pipeline.state(), crate::types::PipelineState::Listening);
    let captured = pipeline.stop_recording_for(operation).unwrap().unwrap();
    assert_eq!(captured.len(), START_AFTER_SAMPLES);
    assert_eq!(
        pipeline.pretranscription_status_for(operation).phase,
        Phase::Inactive
    );
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(batch.results.len(), 1);
}

#[test]
fn capture_worker_failure_is_reported_without_losing_the_full_recording() {
    use super::super::pretranscription::Phase;
    let pipeline = pipeline(1);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let length = pipeline.recording_end() as usize;
    pipeline
        .start_capture_segments_for(operation, vad_path(), 0, |_, _, _, _| {
            Err(AppError::Stt("controlled failure".into()))
        })
        .unwrap();
    wait_until(|| pipeline.pretranscription_status_for(operation).phase == Phase::Fallback);
    let captured = pipeline.stop_recording_for(operation).unwrap().unwrap();
    assert_eq!(captured.len(), length);
    assert!(pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .is_err());
}

#[test]
fn capture_consumer_keeps_an_ordered_private_prefix_and_leaves_the_tail_to_stop() {
    let pipeline = pipeline(2);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let original_length = pipeline.recording_end() as usize;
    let (calls_tx, calls_rx) = mpsc::channel();
    assert!(pipeline
        .start_capture_segments_for(
            operation,
            vad_path(),
            0,
            move |samples, start, context, _| {
                calls_tx.send((start, context.map(str::to_owned))).unwrap();
                Ok(transcript(samples.len(), "phrase"))
            }
        )
        .unwrap());
    wait_until(|| pipeline.completed_capture_segments_for(operation) == Some(2));
    assert!(
        pipeline.take_capture_segments_for(operation).is_none(),
        "cannot consume a running capture"
    );
    let captured = pipeline.stop_recording_for(operation).unwrap().unwrap();
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(batch.operation_id, operation);
    assert_eq!(batch.results.len(), 2);
    let mut next = 0;
    for (index, result) in batch.results.iter().enumerate() {
        assert_eq!(result.operation_id, operation);
        assert_eq!(result.index, index);
        assert_eq!(result.range.start, next);
        assert_eq!(result.transcript.as_ref().unwrap().text, "phrase");
        assert_eq!(result.transcript.as_ref().unwrap().segments.capacity(), 0);
        assert_eq!(result.transcript.as_ref().unwrap().words.capacity(), 0);
        next = result.range.end;
    }
    assert_eq!(batch.processed_until, next);
    assert_eq!(captured.len(), original_length);
    assert!(batch.processed_until < captured.len());
    assert_eq!(
        calls_rx.try_iter().collect::<Vec<_>>(),
        vec![
            (0, None),
            (batch.results[1].range.start as u64, Some("phrase".into()))
        ]
    );
}

#[test]
fn an_interactive_request_retries_only_the_uncommitted_capture_segment() {
    let pipeline = pipeline(2);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let observed = attempts.clone();
    pipeline
        .start_capture_segments_for(
            operation,
            vad_path(),
            0,
            move |samples, start, _, cancellation| {
                let attempt = {
                    let mut values = observed.lock();
                    values.push(start);
                    values.len()
                };
                if attempt == 2 {
                    started_tx.send(()).unwrap();
                    wait_until(|| cancellation.is_cancelled());
                    return Err(AppError::Cancelled("window preempted".into()));
                }
                Ok(transcript(samples.len(), "phrase"))
            },
        )
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let permit = pipeline
        .scheduler()
        .acquire(true, &OperationCancellation::default())
        .unwrap();
    assert_eq!(pipeline.completed_capture_segments_for(operation), Some(1));
    drop(permit);
    wait_until(|| pipeline.completed_capture_segments_for(operation) == Some(2));
    pipeline.stop_recording_for(operation).unwrap();
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    let observed = attempts.lock();
    assert_eq!(observed.len(), 3);
    assert_eq!(observed[0], 0);
    assert_eq!(observed[1], observed[2]);
    assert_eq!(batch.results.len(), 2);
}

#[test]
fn cancelled_non_cooperative_work_keeps_admission_and_cannot_publish_to_a_new_capture() {
    let pipeline = pipeline(1);
    let old = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    pipeline
        .start_capture_segments_for(old, vad_path(), 0, move |samples, _, _, _| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(transcript(samples.len(), "obsolete"))
        })
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    pipeline.cancel_for(old).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    let current = pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let current_cancel = pipeline.cancellation(current).unwrap();
    assert!(pipeline.take_capture_segments_for(old).is_none());
    assert_eq!(pipeline.completed_capture_segments_for(old), None);
    assert!(!pipeline.capture_segments.0.lock()[&old]
        .handle
        .as_ref()
        .unwrap()
        .is_finished());
    assert!(
        activity::is_busy(),
        "actual backend work must retain its lease"
    );
    assert_eq!(pipeline.capture_segments.1.load(Ordering::Acquire), 1);
    assert!(pipeline.scheduler().is_busy());
    release_tx.send(()).unwrap();
    wait_until(|| {
        pipeline.capture_segments.0.lock()[&old]
            .handle
            .as_ref()
            .unwrap()
            .is_finished()
    });
    assert!(matches!(
        pipeline.capture_segments.take(old).unwrap().finish(),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(pipeline.capture_segments.1.load(Ordering::Acquire), 0);
    assert!(!current_cancel.is_cancelled());
    assert!(pipeline.is_operation_active(current));
    pipeline.stop_recording_for(current).unwrap();
    pipeline.cancel_for(current);
}

#[test]
fn stop_preempts_an_unfinished_window_and_returns_only_the_committed_prefix() {
    let pipeline = pipeline(2);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let mut calls = 0;
    pipeline
        .start_capture_segments_for(
            operation,
            vad_path(),
            0,
            move |samples, _, _, cancellation| {
                calls += 1;
                if calls == 2 {
                    started_tx.send(()).unwrap();
                    wait_until(|| cancellation.is_cancelled());
                    return Ok(transcript(samples.len(), "late result"));
                }
                Ok(transcript(samples.len(), "ready prefix"))
            },
        )
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    pipeline.stop_recording_for(operation).unwrap();
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(batch.results.len(), 1);
    assert_eq!(
        batch.results[0].transcript.as_ref().unwrap().text,
        "ready prefix"
    );
    assert_eq!(batch.processed_until, batch.results[0].range.end);
    assert!(pipeline
        .scheduler()
        .acquire_capture(operation, &OperationCancellation::default())
        .is_err());
    let _interactive = pipeline
        .scheduler()
        .acquire(true, &OperationCancellation::default())
        .unwrap();
}

#[test]
fn forced_silence_ranges_never_reach_stt() {
    let pipeline = Pipeline::new_with_audio_owner(Box::new(Recorder(vec![0; 26 * 16_000])));
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    pipeline
        .start_capture_segments_for(operation, vad_path(), 0, |_, _, _, _| {
            panic!("silence must be rejected before STT")
        })
        .unwrap();
    wait_until(|| pipeline.completed_capture_segments_for(operation) == Some(1));
    let samples = pipeline.stop_recording_for(operation).unwrap().unwrap();
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    assert!(batch.results[0].transcript.is_none());
    assert_eq!(batch.processed_until, 25 * 16_000);
    assert_eq!(samples.len(), 26 * 16_000);
}

#[test]
fn an_oversized_result_leaves_its_pcm_for_the_stop_fallback() {
    let pipeline = pipeline(1);
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    pipeline
        .start_capture_segments_for(operation, vad_path(), 0, |samples, _, _, _| {
            Ok(transcript(samples.len(), &"x".repeat(MAX_TEXT_BYTES + 1)))
        })
        .unwrap();
    wait_until(|| {
        pipeline.capture_segments.0.lock()[&operation]
            .handle
            .as_ref()
            .unwrap()
            .is_finished()
    });
    let samples = pipeline.stop_recording_for(operation).unwrap().unwrap();
    let batch = pipeline
        .take_capture_segments_for(operation)
        .unwrap()
        .finish()
        .unwrap();
    assert!(batch.results.is_empty());
    assert_eq!(batch.processed_until, 0);
    assert!(!samples.is_empty());
}

#[test]
fn a_dropped_stop_waiter_keeps_its_actual_worker_slot_until_backend_exit() {
    let pipeline = pipeline(1);
    let old = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    pipeline
        .start_capture_segments_for(old, vad_path(), 0, move |samples, _, _, _| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(transcript(samples.len(), "late"))
        })
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    drop(pipeline.take_capture_segments_for(old).unwrap());
    pipeline.cancel_for(old).unwrap();
    let current = pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    pipeline
        .start_capture_segments_for(current, vad_path(), 0, |_, _, _, _| {
            panic!("second capture must wait for the actual backend")
        })
        .unwrap();
    assert_eq!(pipeline.capture_segments.0.lock().len(), 1);
    assert_eq!(
        pipeline.capture_segments.1.load(Ordering::Acquire),
        MAX_TASKS
    );
    assert!(activity::is_busy());
    // The old handle has left the registry, but its still-running backend
    // continues to count against the bound. No third worker may be spawned.
    let extra = CaptureSegmentInput {
        operation: current + 1,
        profile: CaptureSegmentProfile::from(&crate::types::Settings::default()),
        cancellation: OperationCancellation::default(),
        writer: pipeline.writer.lock().as_ref().unwrap().clone(),
        scheduler: pipeline.scheduler(),
        vad_path: vad_path(),
        start_after_samples: 0,
    };
    assert!(matches!(
        pipeline.capture_segments.start(
            extra,
            |_, _, _, _| { panic!("worker capacity must be checked before spawning") },
            activity::lease().unwrap()
        ),
        Err(AppError::Busy(_))
    ));
    pipeline.cancel_for(current).unwrap();
    pipeline.stop_recording_for(current).unwrap();
    release_tx.send(()).unwrap();
    wait_until(|| pipeline.capture_segments.1.load(Ordering::Acquire) == 0);
    assert!(matches!(
        pipeline.capture_segments.take(current).unwrap().finish(),
        Err(AppError::Cancelled(_))
    ));
}
