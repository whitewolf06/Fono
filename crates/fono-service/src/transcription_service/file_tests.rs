use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::*;

#[derive(Default)]
struct FileRuntime {
    configured_calls: AtomicUsize,
    files: Mutex<Vec<TranscriptionRequest>>,
}

fn request() -> TranscriptionRequest {
    TranscriptionRequest {
        pcm_samples: vec![1, -2, 3],
        language: "ru".into(),
        model: None,
    }
}

fn result() -> TranscriptionResult {
    TranscriptionResult {
        protocol_version: fono_types::TRANSCRIPTION_PROTOCOL_VERSION,
        text: "remote file".into(),
        detected_language: Some("ru".into()),
        audio_seconds: Some(0.5),
        transcribe_seconds: None,
        model: "whisper:tiny".into(),
        backend: Some("remote:cuda:0".into()),
        segments: vec![SubtitleSegment {
            start_ms: 0,
            end_ms: 500,
            text: "remote file".into(),
        }],
    }
}

impl TranscriptionRuntime for FileRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        self.configured_calls.fetch_add(1, Ordering::Relaxed);
        Err(TranscriptionServiceError::ModelNotReady(
            "API requires a local source".into(),
        ))
    }

    fn transcribe(
        &self,
        _: &[i16],
        _: &str,
        _: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        panic!("API inference must remain unavailable for this runtime")
    }

    fn transcribe_file_with_progress(
        &self,
        request: TranscriptionRequest,
        _: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> TranscriptionServiceResult<TranscriptionResult> {
        self.files.lock().unwrap().push(request);
        progress(0.5);
        Ok(result())
    }
}

#[test]
fn host_file_uses_its_hook_without_touching_the_local_api_model() {
    let service = TranscriptionService::new(FileRuntime::default());
    let progress = Mutex::new(Vec::new());
    let actual = service
        .transcribe_file_with_progress(request(), OperationCancellation::default(), &|value| {
            progress.lock().unwrap().push(value);
        })
        .unwrap();

    assert_eq!(actual, result());
    assert_eq!(&*service.runtime.files.lock().unwrap(), &[request()]);
    assert_eq!(service.runtime.configured_calls.load(Ordering::Relaxed), 0);
    assert_eq!(&*progress.lock().unwrap(), &[0.5]);
}

#[test]
fn api_request_cannot_select_the_file_hook_with_a_model_or_extra_json_field() {
    let service = TranscriptionService::new(FileRuntime::default());
    let forged: TranscriptionRequest = serde_json::from_value(serde_json::json!({
        "pcm_samples": [1, -2, 3],
        "language": "ru",
        "model": "whisper:tiny",
        "origin": "file"
    }))
    .unwrap();

    assert!(matches!(
        service.transcribe(forged),
        Err(TranscriptionServiceError::ModelNotReady(_))
    ));
    assert!(service.runtime.files.lock().unwrap().is_empty());
    assert_eq!(service.runtime.configured_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn invalid_or_pre_cancelled_file_never_reaches_the_runtime_hook() {
    let service = TranscriptionService::new(FileRuntime::default());
    for invalid in [
        TranscriptionRequest {
            pcm_samples: Vec::new(),
            ..request()
        },
        TranscriptionRequest {
            language: " ".into(),
            ..request()
        },
        TranscriptionRequest {
            model: Some(" ".into()),
            ..request()
        },
    ] {
        assert!(matches!(
            service.transcribe_file_with_progress(
                invalid,
                OperationCancellation::default(),
                &|_| {}
            ),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
    }
    let cancellation = OperationCancellation::default();
    cancellation.cancel();
    assert!(matches!(
        service.transcribe_file_with_progress(request(), cancellation, &|_| {}),
        Err(TranscriptionServiceError::Cancelled(_))
    ));
    assert!(service.runtime.files.lock().unwrap().is_empty());
    assert_eq!(service.runtime.configured_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn default_file_hook_keeps_the_selected_model_progress_and_subtitles() {
    struct LocalRuntime;

    impl TranscriptionRuntime for LocalRuntime {
        type Error = TranscriptionServiceError;

        fn configured_model(&self) -> Result<String, Self::Error> {
            Ok("base".into())
        }

        fn transcribe(
            &self,
            _: &[i16],
            _: &str,
            _: OperationCancellation,
        ) -> Result<Transcript, Self::Error> {
            panic!("selected model must go through the same native path")
        }

        fn transcribe_selected_with_progress(
            &self,
            model: &str,
            pcm: &[i16],
            language: &str,
            _: OperationCancellation,
            progress: &(dyn Fn(f32) + Sync),
        ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
            assert_eq!(model, "base");
            assert_eq!(pcm, &[1, -2, 3]);
            assert_eq!(language, "ru");
            progress(0.75);
            Ok((
                Transcript {
                    remote: None,
                    text: "local file".into(),
                    detected_language: Some("ru".into()),
                    transcribe_secs: Some(0.1),
                    audio_secs: Some(0.5),
                    device: Some("CPU".into()),
                },
                vec![SubtitleSegment {
                    start_ms: 0,
                    end_ms: 500,
                    text: "local file".into(),
                }],
            ))
        }
    }

    let service = TranscriptionService::new(LocalRuntime);
    let progress = Mutex::new(Vec::new());
    let result = service
        .transcribe_file_with_progress(request(), OperationCancellation::default(), &|value| {
            progress.lock().unwrap().push(value);
        })
        .unwrap();
    assert_eq!(result.model, "base");
    assert_eq!(result.backend.as_deref(), Some("CPU"));
    assert_eq!(result.segments.len(), 1);
    assert_eq!(result.segments[0].end_ms, 500);
    assert_eq!(&*progress.lock().unwrap(), &[0.75]);

    assert!(matches!(
        service.transcribe_file_with_progress(
            TranscriptionRequest {
                model: Some("another-model".into()),
                ..request()
            },
            OperationCancellation::default(),
            &|_| {}
        ),
        Err(TranscriptionServiceError::InvalidRequest(_))
    ));
}
