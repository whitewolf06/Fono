use super::*;
use crate::remote::RemoteTranscriptionClient;
use std::sync::atomic::AtomicUsize;

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    {
        let mut metadata = fixture.metadata.lock().unwrap();
        metadata.0["audio_formats"] = json!([PCM_TYPE, "audio/wav"]);
        metadata.0["features"]["jobs"] = json!(true);
    }
    fixture
}

fn client(fixture: &Fixture) -> RemoteTranscriptionClient {
    RemoteTranscriptionClient::connect(
        fixture.endpoint(),
        model(),
        Arc::new(crate::NoopActivity),
        &OperationCancellation::default(),
    )
    .unwrap()
}

fn job(state: &str) -> Value {
    json!({"id":"fixture-job", "requested_language":"en", "state":state,
        "progress":null, "result":null, "error":null})
}

fn result() -> Value {
    json!({"protocol_version":1, "text":"fixture transcript", "detected_language":"en",
        "audio_seconds":0.1, "transcribe_seconds":0.01, "model":"whisper:tiny", "backend":"CUDA",
        "segments":[{"start_ms":0,"end_ms":100,"text":"fixture transcript"}]})
}

fn completed() -> Value {
    let mut job = job("completed");
    job["result"] = result();
    job
}

#[test]
fn file_job_uses_multipart_batch_queue_and_preserves_real_timestamps() {
    let fixture = fixture();
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path == "/v1/transcriptions" {
            assert!(request.headers.starts_with("POST "));
            (202, job("queued"), Duration::ZERO)
        } else {
            assert_eq!(request.path, "/v1/transcription-jobs/fixture-job");
            assert!(request.headers.starts_with("GET "));
            let body = if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                let mut body = job("transcribing");
                body["progress"] = json!(0.5);
                body
            } else {
                completed()
            };
            (200, body, Duration::ZERO)
        }
    }));
    let client = client(&fixture);
    let progress = Mutex::new(Vec::new());
    let samples: Vec<i16> = (0..1600).map(|value| value - 800).collect();
    let result = client
        .transcribe_pcm_job(
            &samples,
            "en",
            &OperationCancellation::default(),
            &|fraction| {
                progress.lock().unwrap().push(fraction);
            },
        )
        .unwrap();
    assert_eq!(result.backend.as_deref(), Some("remote:cuda"));
    assert_eq!(result.model, "whisper:tiny");
    assert_eq!(result.segments[0].end_ms, 100);
    assert_eq!(*progress.lock().unwrap(), vec![0.5, 1.0]);
    assert_eq!(polls.load(Ordering::Acquire), 2);
    let requests = fixture.requests.lock().unwrap();
    assert!(!requests
        .iter()
        .any(|request| request.path.starts_with("/v1/recognize")));
    let upload = requests
        .iter()
        .find(|request| request.path == "/v1/transcriptions")
        .unwrap();
    assert!(upload
        .headers
        .to_ascii_lowercase()
        .contains("multipart/form-data; boundary=fono-"));
    assert!(upload.headers.contains(TOKEN));
    let pcm_start = upload
        .body
        .windows(4)
        .position(|bytes| bytes == b"RIFF")
        .unwrap();
    assert_eq!(&upload.body[pcm_start + 8..pcm_start + 12], b"WAVE");
    assert_eq!(
        &upload.body[pcm_start + 44..pcm_start + 48],
        &[0xe0, 0xfc, 0xe1, 0xfc]
    );
    let multipart = String::from_utf8_lossy(&upload.body[..pcm_start]);
    assert!(multipart.contains("filename=\"audio.wav\""));
    assert!(multipart.contains("whisper:tiny"));
}

#[test]
fn file_job_capabilities_and_bounds_fail_before_audio_upload() {
    for change in [
        |caps: &mut Value| caps["features"]["jobs"] = json!(false),
        |caps: &mut Value| {
            caps["features"].as_object_mut().unwrap().remove("jobs");
        },
        |caps: &mut Value| caps["audio_formats"] = json!([PCM_TYPE]),
        |caps: &mut Value| caps["api_version"] = json!(1),
    ] {
        let fixture = fixture();
        change(&mut fixture.metadata.lock().unwrap().0);
        let error = RemoteTranscriptionClient::connect(
            fixture.endpoint(),
            model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap_err();
        assert_eq!(kind(error).kind, RemoteErrorKind::Incompatible);
        assert!(!fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.path == "/v1/transcriptions"));
    }
    let fixture = fixture();
    fixture.metadata.lock().unwrap().0["max_upload_bytes"] = json!(100);
    let client = client(&fixture);
    assert!(client
        .transcribe_pcm_job(&[0; 29], "en", &OperationCancellation::default(), &|_| {})
        .is_err());
    for language in ["", "unsupported", "en\r\ninjected"] {
        assert!(client
            .transcribe_pcm_job(
                &[0; 16],
                language,
                &OperationCancellation::default(),
                &|_| {}
            )
            .is_err());
    }
    assert!(!fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.path == "/v1/transcriptions"));
}

#[test]
fn file_job_does_not_apply_interactive_64_mib_limit() {
    let fixture = fixture();
    {
        let mut metadata = fixture.metadata.lock().unwrap();
        metadata.0["max_upload_bytes"] = json!(100 * 1024 * 1024);
        metadata.0["max_audio_seconds"] = json!(7200);
    }
    // Assert the admission plan without allocating/uploading a large fixture.
    let client = client(&fixture);
    assert!(client.max_samples > (64 * 1024 * 1024) / 2);
    assert_eq!(client.max_samples, (100 * 1024 * 1024 - 44) / 2);
}

#[test]
fn cancel_while_submit_response_is_drained_still_cancels_known_job() {
    let fixture = fixture();
    let cancellation = OperationCancellation::default();
    let observed_cancellation = cancellation.clone();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path == "/v1/transcriptions" {
            observed_cancellation.cancel();
            (202, job("queued"), Duration::from_millis(80))
        } else {
            assert_eq!(request.path, "/v1/transcription-jobs/fixture-job/cancel");
            assert!(request.headers.starts_with("POST "));
            (200, job("cancelled"), Duration::ZERO)
        }
    }));
    let client = client(&fixture);
    assert!(matches!(
        client.transcribe_pcm_job(&[0; 1600], "en", &cancellation, &|_| {}),
        Err(AppError::Cancelled(_))
    ));
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.path.ends_with("/cancel")));
}

#[test]
fn cancel_after_progress_retains_lifecycle_admission_until_cleanup_finishes() {
    let fixture = fixture();
    let activity = Arc::new(TestActivity::default());
    let cancellation = OperationCancellation::default();
    let checked = activity.clone();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path == "/v1/transcriptions" {
            let mut body = job("transcribing");
            body["progress"] = json!(0.25);
            (202, body, Duration::ZERO)
        } else {
            assert_eq!(request.path, "/v1/transcription-jobs/fixture-job/cancel");
            assert!(checked.is_busy());
            (200, job("cancelled"), Duration::from_millis(30))
        }
    }));
    let client = RemoteTranscriptionClient::connect(
        fixture.endpoint(),
        model(),
        activity.clone(),
        &OperationCancellation::default(),
    )
    .unwrap();
    let observed = cancellation.clone();
    let error = client
        .transcribe_pcm_job(&[0; 1600], "en", &cancellation, &|_| {
            assert!(activity.is_busy());
            observed.cancel();
        })
        .unwrap_err();
    assert!(matches!(error, AppError::Cancelled(_)));
    assert!(!activity.is_busy());
}

#[test]
fn invalid_or_changed_job_id_cannot_escape_the_fixed_route() {
    for malicious in [
        "../PRIVATE",
        "https://PRIVATE",
        "foo?token=PRIVATE",
        "",
        "a/b",
    ] {
        let fixture = fixture();
        let malformed = malicious.to_owned();
        *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |_| {
            let mut body = job("queued");
            body["id"] = json!(malformed);
            (202, body, Duration::ZERO)
        }));
        let error = client(&fixture)
            .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
            .unwrap_err();
        assert_eq!(kind(error).kind, RemoteErrorKind::InvalidResponse);
        assert!(!fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.path.starts_with("/v1/transcription-jobs/")));
    }
    let fixture = fixture();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        let mut body = job("queued");
        if request.path.ends_with("/cancel") {
            return (200, job("cancelled"), Duration::ZERO);
        }
        if request.path != "/v1/transcriptions" {
            body["id"] = json!("other-id");
        }
        (
            if request.path == "/v1/transcriptions" {
                202
            } else {
                200
            },
            body,
            Duration::ZERO,
        )
    }));
    assert_eq!(
        kind(
            client(&fixture)
                .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
    let requests = fixture.requests.lock().unwrap();
    assert!(requests
        .iter()
        .any(|request| request.path == "/v1/transcription-jobs/fixture-job/cancel"));
    assert!(!requests
        .iter()
        .any(|request| request.path.contains("other-id")));
}

#[test]
fn invalid_terminal_facts_and_server_messages_never_escape_into_results_or_errors() {
    for change in [
        |value: &mut Value| value["model"] = json!("PRIVATE_model"),
        |value: &mut Value| value["backend"] = json!("PRIVATE_device_name"),
        |value: &mut Value| value["protocol_version"] = json!(99),
        |value: &mut Value| value["audio_seconds"] = json!(10000),
        |value: &mut Value| value["transcribe_seconds"] = json!(-1),
        |value: &mut Value| value["segments"][0]["end_ms"] = json!(101),
    ] {
        let fixture = fixture();
        let mut reply = completed();
        change(&mut reply["result"]);
        *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
            if request.path.ends_with("/cancel") {
                (200, job("cancelled"), Duration::ZERO)
            } else {
                (202, reply.clone(), Duration::ZERO)
            }
        }));
        let error = client(&fixture)
            .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
            .unwrap_err();
        assert!(!format!("{error:?}").contains("PRIVATE"));
        assert_eq!(kind(error).kind, RemoteErrorKind::InvalidResponse);
    }
    let fixture = fixture();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path.ends_with("/cancel") {
            return (404, json!({"error":"PRIVATE_token"}), Duration::ZERO);
        }
        let mut failed = job("failed");
        failed["error"] = json!({"code":"worker_failed", "message":"PRIVATE_token audio text"});
        (202, failed, Duration::ZERO)
    }));
    let error = client(&fixture)
        .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
        .unwrap_err();
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert_eq!(kind(error).kind, RemoteErrorKind::WorkerFailed);
}

#[test]
fn incomplete_job_replies_unknown_durations_and_empty_timestamps_remain_compatible() {
    let fixture = fixture();
    let mut reply = completed();
    for key in ["backend", "audio_seconds", "transcribe_seconds"] {
        reply["result"][key] = Value::Null;
    }
    reply["result"].as_object_mut().unwrap().remove("segments");
    *fixture.mode.lock().unwrap() =
        Mode::Handler(Arc::new(move |_| (202, reply.clone(), Duration::ZERO)));
    let result = client(&fixture)
        .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
        .unwrap();
    assert!(result.backend.is_none());
    assert!(result.audio_seconds.is_none());
    assert!(result.transcribe_seconds.is_none());
    assert!(result.segments.is_empty());
}

#[test]
fn upload_requires_accepted_and_poll_requires_ok() {
    for wrong_upload_status in [200, 201] {
        let fixture = fixture();
        *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |_| {
            (wrong_upload_status, completed(), Duration::ZERO)
        }));
        assert_eq!(
            kind(
                client(&fixture)
                    .transcribe_pcm_job(
                        &[0; 1600],
                        "en",
                        &OperationCancellation::default(),
                        &|_| {}
                    )
                    .unwrap_err()
            )
            .kind,
            RemoteErrorKind::InvalidResponse
        );
    }
    let fixture = fixture();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path.ends_with("/cancel") {
            (200, job("cancelled"), Duration::ZERO)
        } else {
            (202, job("queued"), Duration::ZERO)
        }
    }));
    assert_eq!(
        kind(
            client(&fixture)
                .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
}

#[test]
fn timeout_cancels_known_job_and_does_not_publish_late_completion() {
    let fixture = fixture();
    let endpoint = fixture
        .endpoint()
        .with_timeouts(RemoteTimeouts {
            connect: Duration::from_secs(1),
            response_base: Duration::from_secs(1),
            audio_multiplier: 0.0,
        })
        .unwrap();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(move |request| {
        if request.path.ends_with("/cancel") {
            (200, job("cancelled"), Duration::ZERO)
        } else if request.path == "/v1/transcriptions" {
            (202, job("queued"), Duration::ZERO)
        } else {
            (200, job("queued"), Duration::ZERO)
        }
    }));
    let client = RemoteTranscriptionClient::connect(
        endpoint,
        model(),
        Arc::new(crate::NoopActivity),
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(
        kind(
            client
                .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Timeout
    );
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.path.ends_with("/cancel")));
    assert!(fixture
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.path == "/v1/transcription-jobs/fixture-job"));
}
