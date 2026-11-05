use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use super::*;
use fono_stt::{error::SttResult, NoopActivity};
use fono_types::Transcript;

#[derive(Default)]
struct RoutedRuntime {
    files: Mutex<Vec<TranscriptionRequest>>,
    api_checks: AtomicUsize,
}

fn file_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/tone-16k-mono.wav")
}

fn file_result() -> TranscriptionResult {
    TranscriptionResult {
        protocol_version: fono_types::TRANSCRIPTION_PROTOCOL_VERSION,
        text: "remote file".into(),
        detected_language: Some("ru".into()),
        audio_seconds: Some(0.25),
        transcribe_seconds: None,
        model: "whisper:tiny".into(),
        backend: Some("remote:cpu".into()),
        segments: Vec::new(),
    }
}

impl TranscriptionRuntime for RoutedRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        self.api_checks.fetch_add(1, Ordering::Relaxed);
        Err(TranscriptionServiceError::ModelNotReady(
            "local API has no local model".into(),
        ))
    }

    fn transcribe(
        &self,
        _: &[i16],
        _: &str,
        _: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        panic!("a saved local model must never be reactivated")
    }

    fn transcribe_file_with_progress(
        &self,
        request: TranscriptionRequest,
        _: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> fono_types::TranscriptionServiceResult<TranscriptionResult> {
        self.files.lock().push(request);
        progress(0.25);
        progress(0.75);
        Ok(file_result())
    }
}

#[test]
fn decoded_host_file_reaches_the_file_hook_and_reports_its_progress() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&events);
    let queue = TranscriptionJobQueue::new(
        TranscriptionService::new(RoutedRuntime::default()),
        NoInteractiveActivity,
        4,
        Arc::new(NoopActivity),
        Arc::new(NoCaptureGate),
    )
    .with_observer(Arc::new(move |job| observed.lock().push(job)));
    let accepted = queue.submit_file(file_fixture(), "ru".into()).unwrap();
    let completed = queue.run_next().unwrap();

    assert_eq!(accepted.id, completed.id);
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.result, Some(file_result()));
    assert_eq!(completed.progress, Some(1.0));
    let files = queue.service.runtime().files.lock();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].language, "ru");
    assert_eq!(files[0].model, None);
    assert!(!files[0].pcm_samples.is_empty());
    assert!(files[0].pcm_samples.iter().any(|sample| *sample != 0));
    assert_eq!(
        queue.service.runtime().api_checks.load(Ordering::Relaxed),
        0
    );
    let events = events.lock();
    assert!(events.iter().any(|job| job.progress == Some(0.25)));
    assert!(events.iter().any(|job| job.progress == Some(0.75)));
    assert_eq!(events.last().unwrap().state, JobState::Completed);
}

#[test]
fn queued_http_pcm_cannot_use_the_host_file_hook() {
    let queue = TranscriptionJobQueue::new(
        TranscriptionService::new(RoutedRuntime::default()),
        NoInteractiveActivity,
        4,
        Arc::new(NoopActivity),
        Arc::new(NoCaptureGate),
    );
    let request = serde_json::from_value(serde_json::json!({
        "pcm_samples": [1, -2, 3],
        "language": "ru",
        "model": "whisper:tiny",
        "origin": "file"
    }))
    .unwrap();
    let queued = queue.submit_for_client("http-client", request).unwrap();
    let failed = queue.run_next().unwrap();
    assert_eq!(queued.id, failed.id);
    assert_eq!(failed.state, JobState::Failed);
    assert!(matches!(
        failed.error,
        Some(TranscriptionServiceError::ModelNotReady(_))
    ));
    assert!(queue.service.runtime().files.lock().is_empty());
    assert_eq!(
        queue.service.runtime().api_checks.load(Ordering::Relaxed),
        1
    );
}

struct Gate(AtomicBool);

impl InteractiveActivity for Gate {
    fn is_active(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[test]
fn interactive_work_defers_host_file_and_pending_cancel_never_opens_it() {
    let queue = TranscriptionJobQueue::new(
        TranscriptionService::new(RoutedRuntime::default()),
        Gate(AtomicBool::new(true)),
        4,
        Arc::new(NoopActivity),
        Arc::new(NoCaptureGate),
    );
    let discarded = queue
        .submit_file(PathBuf::from("private/not-present.wav"), "ru".into())
        .unwrap();
    let ready = queue.submit_file(file_fixture(), "ru".into()).unwrap();
    assert!(queue.run_next().is_none());
    assert!(queue.service.runtime().files.lock().is_empty());
    assert_eq!(queue.get(&ready.id).unwrap().state, JobState::Queued);

    queue.cancel(&discarded.id).unwrap();
    queue.interactive.0.store(false, Ordering::Release);
    assert_eq!(queue.run_next().unwrap().id, ready.id);
    assert_eq!(queue.get(&discarded.id).unwrap().state, JobState::Cancelled);
    assert_eq!(queue.service.runtime().files.lock().len(), 1);
}

#[derive(Clone, Default)]
struct CountingActivity(Arc<AtomicUsize>);

impl ActivityGuard for CountingActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        Ok(self.cleanup_lease())
    }

    fn cleanup_lease(&self) -> ActivityLease {
        struct Token(Arc<AtomicUsize>);
        impl Drop for Token {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        self.0.fetch_add(1, Ordering::AcqRel);
        ActivityLease::new(Token(Arc::clone(&self.0)))
    }
}

#[test]
fn cancelled_file_keeps_admission_and_ignores_late_progress_and_success() {
    struct BlockingFile {
        started: mpsc::Sender<()>,
        finish: Mutex<mpsc::Receiver<()>>,
    }

    impl TranscriptionRuntime for BlockingFile {
        type Error = TranscriptionServiceError;

        fn configured_model(&self) -> Result<String, Self::Error> {
            panic!("file hook must not use the API model")
        }

        fn transcribe(
            &self,
            _: &[i16],
            _: &str,
            _: OperationCancellation,
        ) -> Result<Transcript, Self::Error> {
            panic!("file hook must not use API inference")
        }

        fn transcribe_file_with_progress(
            &self,
            request: TranscriptionRequest,
            cancellation: OperationCancellation,
            progress: &(dyn Fn(f32) + Sync),
        ) -> fono_types::TranscriptionServiceResult<TranscriptionResult> {
            assert!(!request.pcm_samples.is_empty());
            progress(0.5);
            self.started.send(()).unwrap();
            self.finish
                .lock()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(cancellation.is_cancelled());
            progress(0.9);
            Ok(file_result())
        }
    }

    let (started_tx, started_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let activity = CountingActivity::default();
    let queue = Arc::new(TranscriptionJobQueue::new(
        TranscriptionService::new(BlockingFile {
            started: started_tx,
            finish: Mutex::new(finish_rx),
        }),
        NoInteractiveActivity,
        4,
        Arc::new(activity.clone()),
        Arc::new(NoCaptureGate),
    ));
    let accepted = queue.submit_file(file_fixture(), "ru".into()).unwrap();
    let worker_queue = Arc::clone(&queue);
    let worker = std::thread::spawn(move || worker_queue.run_next().unwrap());
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(queue.get(&accepted.id).unwrap().progress, Some(0.5));

    queue.cancel(&accepted.id).unwrap();
    assert_eq!(queue.get(&accepted.id).unwrap().state, JobState::Cancelled);
    assert!(activity.0.load(Ordering::Acquire) > 0);
    assert!(queue.state.lock().work.contains_key(&accepted.id));
    finish_tx.send(()).unwrap();
    let terminal = worker.join().unwrap();
    assert_eq!(terminal.state, JobState::Cancelled);
    assert_eq!(terminal.progress, Some(0.5));
    assert!(terminal.result.is_none());
    assert!(queue.state.lock().work.is_empty());
    assert_eq!(activity.0.load(Ordering::Acquire), 0);
}
