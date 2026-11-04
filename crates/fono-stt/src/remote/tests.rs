use super::*;
mod inspection_tests;
use crate::error::AppError;
use crate::ports::TestActivity;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const TOKEN: &str = "PRIVATE_fixture_bearer";
fn model() -> ModelId {
    ModelId {
        engine: ModelEngine::Whisper,
        variant: "tiny".into(),
    }
}
fn options() -> RecognitionOptions<'static> {
    RecognitionOptions {
        language: "en",
        decoding: Default::default(),
        glossary: &[],
    }
}

#[derive(Clone)]
enum Mode {
    Ok,
    Status(u16),
    Hang,
    SlowBody,
    Disconnect,
    Redirect(String),
    Reply(Value),
    HealthReply(u16, Value),
    Oversized,
}
struct Capture {
    path: String,
    headers: String,
    body: Vec<u8>,
}
pub(crate) struct Fixture {
    endpoint: String,
    mode: Arc<Mutex<Mode>>,
    requests: Arc<Mutex<Vec<Capture>>>,
    metadata: Arc<Mutex<(Value, Value)>>,
    disconnected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let mode = Arc::new(Mutex::new(Mode::Ok));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let metadata = Arc::new(Mutex::new((capabilities(), models())));
        let disconnected = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (m, r, d, s) = (
            mode.clone(),
            requests.clone(),
            disconnected.clone(),
            stop.clone(),
        );
        let server_metadata = metadata.clone();
        let worker = thread::spawn(move || {
            let mut connections = Vec::new();
            while !s.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (m, r, d, s) = (m.clone(), r.clone(), d.clone(), s.clone());
                        let metadata = server_metadata.clone();
                        connections
                            .push(thread::spawn(move || serve(stream, m, r, d, s, metadata)));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            endpoint,
            mode,
            requests,
            metadata,
            disconnected,
            stop,
            worker: Some(worker),
        }
    }
    fn client(&self, activity: Arc<dyn ActivityGuard>) -> RemoteRecognizer {
        RemoteRecognizer::connect(
            RemoteEndpoint::new(&self.endpoint, TOKEN).unwrap(),
            model(),
            activity,
            &OperationCancellation::default(),
        )
        .unwrap()
    }
    pub(crate) fn endpoint(&self) -> RemoteEndpoint {
        RemoteEndpoint::new(&self.endpoint, TOKEN).unwrap()
    }
    fn recognition_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with("/v1/recognize?"))
            .count()
    }
    fn wait_recognition(&self, count: usize) {
        wait(|| self.recognition_count() >= count);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn wait(condition: impl Fn() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "fixture condition timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn capabilities() -> Value {
    json!({"product":"fono-server","version":"0.8.30","devices":[{"id":"cpu","kind":"cpu"},{"id":"cuda:0","kind":"cuda","name":"NVIDIA GeForce RTX 3060 Ti"}],"api_version":2,"audio_formats":[PCM_TYPE],"max_audio_seconds":300,
        "max_upload_bytes":10485760,"features":{"cancel_on_disconnect":true,"glossary":true,"timestamps":["none","segments","words"],"decoding":["fast","accurate"]}})
}
fn models() -> Value {
    json!({"object":"list","data":[{"id":"whisper:tiny","object":"model","fono":{
        "engine":"whisper","languages":["en","ru","auto"],"disk_state":"verified","state":"ready","device":"cuda:0",
        "features":{"glossary":true,"decoding":["fast","accurate"],"timestamps":["none","segments","words"]}}}]})
}
fn reply(samples: usize) -> Value {
    json!({"text":"fixture transcript","language":"en","model":"whisper:tiny","backend":"cuda","device":"cuda:0",
        "segments":[{"start_ms":0,"end_ms":samples / 16,"text":"fixture transcript"}],"timings":{"audio_ms":samples / 16,"total_ms":25}})
}
fn health(server: &str, state: &str) -> Value {
    json!({"status":server,"models":[{"id":"whisper:tiny","state":state,"device":"PRIVATE_device_name"}],
        "queue":{"interactive":2,"batch":3}})
}
fn read_request(stream: &mut TcpStream, stop: &AtomicBool) -> Option<Capture> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if stop.load(Ordering::Acquire) {
                    return None;
                }
                continue;
            }
            Err(_) => return None,
            _ => {}
        }
        bytes.push(byte[0]);
        if bytes.len() > 16384 {
            return None;
        }
    }
    let headers = String::from_utf8(bytes).ok()?;
    let path = headers
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .to_owned();
    let length = headers
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or_default();
    if length > 1024 * 1024 {
        return None;
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).ok()?;
    Some(Capture {
        path,
        headers,
        body,
    })
}
fn respond(stream: &mut impl Write, status: u16, body: &str, extra: &str) {
    write!(stream, "HTTP/1.1 {status} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}
fn serve(
    mut stream: TcpStream,
    mode: Arc<Mutex<Mode>>,
    requests: Arc<Mutex<Vec<Capture>>>,
    disconnected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    metadata: Arc<Mutex<(Value, Value)>>,
) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    while !stop.load(Ordering::Acquire) {
        let Some(request) = read_request(&mut stream, &stop) else {
            return;
        };
        let path = request.path.clone();
        let samples = request.body.len() / 2;
        requests.lock().unwrap().push(request);
        if path == "/v1/capabilities" || path == "/v1/models" {
            if let Mode::Status(status) = *mode.lock().unwrap() {
                respond(&mut stream, status, "PRIVATE_metadata_error", "");
                continue;
            }
            let data = metadata.lock().unwrap();
            respond(
                &mut stream,
                200,
                &if path == "/v1/capabilities" {
                    &data.0
                } else {
                    &data.1
                }
                .to_string(),
                "",
            );
            continue;
        }
        let action = mode.lock().unwrap().clone();
        match action {
            Mode::Ok => respond(
                &mut stream,
                200,
                &if path == "/v1/health" {
                    health("ready", "ready")
                } else {
                    reply(samples)
                }
                .to_string(),
                "",
            ),
            Mode::Status(status) => respond(
                &mut stream,
                status,
                "PRIVATE_server_error_token_and_audio",
                "Retry-After: 3\r\n",
            ),
            Mode::Reply(value) => respond(&mut stream, 200, &value.to_string(), ""),
            Mode::HealthReply(status, value) => {
                respond(&mut stream, status, &value.to_string(), "")
            }
            Mode::Oversized => {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 999999999\r\n\r\n");
            }
            Mode::Redirect(url) => respond(&mut stream, 307, "", &format!("Location: {url}\r\n")),
            Mode::Disconnect => return,
            Mode::Hang | Mode::SlowBody => {
                if matches!(action, Mode::SlowBody) {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32768\r\n\r\n{").unwrap();
                }
                while !stop.load(Ordering::Acquire) {
                    let mut byte = [0];
                    match stream.read(&mut byte) {
                        Ok(0) => {
                            disconnected.store(true, Ordering::Release);
                            return;
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => {
                            disconnected.store(true, Ordering::Release);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
fn kind(error: AppError) -> RemoteError {
    match error {
        AppError::Remote(e) => e,
        _ => panic!("expected classified remote error"),
    }
}

#[test]
fn secure_endpoint_policy_and_debug_never_include_credentials() {
    for url in [
        "http://192.168.1.8:17832",
        "http://example.com",
        "ftp://127.0.0.1",
        "https://user:PRIVATE@example.com",
        "https://host/v1/",
        "https://host?token=PRIVATE",
        "https://host/#PRIVATE",
    ] {
        let error = RemoteEndpoint::new(url, TOKEN).unwrap_err();
        assert_eq!(kind(error).kind, RemoteErrorKind::InvalidEndpoint);
    }
    for url in [
        "https://kari.example",
        "http://127.0.0.1:17833",
        "http://[::1]:17833",
        "http://localhost:17833",
    ] {
        let endpoint = RemoteEndpoint::new(url, TOKEN).unwrap();
        assert!(!format!("{endpoint:?}").contains(TOKEN));
    }
    assert!(RemoteEndpoint::new("https://example.com", "bad\r\nheader").is_err());
}

#[test]
fn discovery_checks_protocol_selected_model_and_feature_limits_before_audio() {
    let server = Fixture::new();
    let connect = || {
        RemoteRecognizer::connect(
            RemoteEndpoint::new(&server.endpoint, TOKEN).unwrap(),
            model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
    };
    server.metadata.lock().unwrap().0["api_version"] = json!(1);
    assert_eq!(
        kind(connect().unwrap_err()).kind,
        RemoteErrorKind::Incompatible
    );
    server.metadata.lock().unwrap().0 = capabilities();
    server.metadata.lock().unwrap().1["data"] = json!([]);
    assert_eq!(
        kind(connect().unwrap_err()).kind,
        RemoteErrorKind::ModelNotFound
    );
    server.metadata.lock().unwrap().1 = models();
    {
        let mut metadata = server.metadata.lock().unwrap();
        metadata.0["max_audio_seconds"] = json!(2);
        metadata.0["max_upload_bytes"] = json!(64000);
        metadata.1["data"][0]["fono"]["features"]["glossary"] = json!(false);
    }
    let client = connect().unwrap();
    assert_eq!(client.capabilities().max_segment_samples, 32000);
    assert!(!client.capabilities().hotwords);
    assert_eq!(server.recognition_count(), 0);
    *server.mode.lock().unwrap() = Mode::Status(401);
    assert_eq!(
        kind(connect().unwrap_err()).kind,
        RemoteErrorKind::Unauthorized
    );
}

#[test]
fn oversized_response_is_rejected_before_reading_the_body() {
    let server = Fixture::new();
    let client = server.client(Arc::new(crate::NoopActivity));
    *server.mode.lock().unwrap() = Mode::Oversized;
    assert_eq!(
        kind(
            client
                .transcribe(&[0; 160], options(), &OperationCancellation::default())
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
}

#[test]
fn repeated_remote_ensure_reuses_connection_but_configuration_changes_replace_it() {
    let server = Fixture::new();
    let engine = crate::SttEngine::default();
    let cancel = OperationCancellation::default();
    engine
        .ensure_remote_loaded(server.endpoint(), &model(), &cancel)
        .unwrap();
    let generation = engine.gpu_residency().generation;
    for _ in 0..3 {
        engine
            .ensure_remote_loaded(server.endpoint(), &model(), &cancel)
            .unwrap();
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(engine.gpu_residency().generation, generation);
    let changed_token = RemoteEndpoint::new(&server.endpoint, "different-test-token").unwrap();
    engine
        .ensure_remote_loaded(changed_token, &model(), &cancel)
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    assert!(engine.gpu_residency().generation > generation);
    let changed_deadline = server
        .endpoint()
        .with_timeouts(RemoteTimeouts {
            response_base: Duration::from_secs(6),
            ..Default::default()
        })
        .unwrap();
    engine
        .ensure_remote_loaded(changed_deadline, &model(), &cancel)
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 6);
}

#[test]
fn failed_or_pre_cancelled_replacement_keeps_previous_remote_usable() {
    let server = Fixture::new();
    let other = Fixture::new();
    let engine = crate::SttEngine::default();
    engine
        .ensure_remote_loaded(
            server.endpoint(),
            &model(),
            &OperationCancellation::default(),
        )
        .unwrap();
    let generation = engine.gpu_residency().generation;
    other.metadata.lock().unwrap().1["data"] = json!([]);
    assert!(engine
        .ensure_remote_loaded(
            other.endpoint(),
            &model(),
            &OperationCancellation::default()
        )
        .is_err());
    let cancel = OperationCancellation::default();
    cancel.cancel();
    assert!(matches!(
        engine.ensure_remote_loaded(other.endpoint(), &model(), &cancel),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(engine.gpu_residency().generation, generation);
    assert!(engine.is_remote());
    assert!(matches!(
        engine.readiness(),
        crate::SttReadiness::Ready { .. }
    ));
    assert_eq!(
        engine.transcribe(&[0; 160], "en").unwrap().text,
        "fixture transcript"
    );
    assert_eq!(other.recognition_count(), 0);
}

#[test]
fn active_remote_operation_blocks_source_change_without_cancelling_work() {
    let server = Fixture::new();
    let other = Fixture::new();
    let engine = Arc::new(crate::SttEngine::default());
    let cancellation = OperationCancellation::default();
    engine
        .ensure_remote_loaded(server.endpoint(), &model(), &cancellation)
        .unwrap();
    *server.mode.lock().unwrap() = Mode::Hang;
    let (owned, token) = (engine.clone(), cancellation.clone());
    let work = thread::spawn(move || owned.transcribe_cancellable(&[0; 160], "en", token));
    server.wait_recognition(1);
    // Exact ensures are harmless during an operation; replacements are not.
    engine
        .ensure_remote_loaded(
            server.endpoint(),
            &model(),
            &OperationCancellation::default(),
        )
        .unwrap();
    assert!(matches!(
        engine.ensure_remote_loaded(
            other.endpoint(),
            &model(),
            &OperationCancellation::default()
        ),
        Err(AppError::Busy(_))
    ));
    assert!(other.requests.lock().unwrap().is_empty());
    assert!(matches!(
        engine.clear_remote_if_idle(),
        Err(AppError::Busy(_))
    ));
    assert!(!cancellation.is_cancelled());
    cancellation.cancel();
    assert!(matches!(work.join().unwrap(), Err(AppError::Cancelled(_))));
    engine
        .ensure_remote_loaded(
            other.endpoint(),
            &model(),
            &OperationCancellation::default(),
        )
        .unwrap();
    assert!(engine.is_remote());
}

#[test]
fn returning_to_unconfigured_local_source_clears_the_remote_route() {
    let server = Fixture::new();
    let engine = crate::SttEngine::default();
    engine
        .ensure_remote_loaded(
            server.endpoint(),
            &model(),
            &OperationCancellation::default(),
        )
        .unwrap();
    engine.clear_remote_if_idle().unwrap();
    assert!(!engine.is_remote());
    assert!(!engine.is_loaded());
    assert_eq!(engine.model(), None);
    assert!(matches!(engine.readiness(), crate::SttReadiness::Unloaded));
    assert!(matches!(
        engine.transcribe(&[0; 160], "en"),
        Err(AppError::ModelNotLoaded)
    ));
    engine.clear_remote_if_idle().unwrap();
}

#[test]
fn real_http_success_uses_network_endian_and_native_glossary() {
    let server = Fixture::new();
    let activity = Arc::new(TestActivity::default());
    let client = server.client(activity.clone());
    let mut samples = vec![0; 16000];
    samples[..4].copy_from_slice(&[0x1234, -2, i16::MIN, i16::MAX]);
    let terms = ["Fono".into(), "Кари".into()];
    let result = client
        .transcribe(
            &samples,
            RecognitionOptions {
                glossary: &terms,
                ..options()
            },
            &OperationCancellation::default(),
        )
        .unwrap();
    assert_eq!(result.text, "fixture transcript");
    assert_eq!(result.device.as_deref(), Some("remote:cuda:0"));
    assert_eq!(result.audio_secs, Some(1.0));
    assert!(!activity.is_busy());
    assert!(client.is_idle());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let request = &requests[2];
    assert!(request.path.contains("model=whisper%3Atiny"));
    assert!(request.path.contains("priority=interactive"));
    assert_eq!(
        &request.body[..8],
        &[0x12, 0x34, 0xff, 0xfe, 0x80, 0, 0x7f, 0xff]
    );
    let headers = request.headers.to_ascii_lowercase();
    assert!(headers.contains("content-type: audio/l16;rate=16000;channels=1"));
    let encoded = request
        .headers
        .lines()
        .find_map(|l| l.strip_prefix("x-fono-glossary: "))
        .unwrap();
    let decoded: Vec<String> =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
    assert_eq!(decoded, terms);
}

#[test]
fn real_http_windows_rebase_timestamps_and_do_not_claim_local_gpu() {
    let server = Fixture::new();
    let engine = crate::SttEngine::default();
    engine
        .connect_remote(
            RemoteEndpoint::new(&server.endpoint, TOKEN).unwrap(),
            model(),
            &OperationCancellation::default(),
        )
        .unwrap();
    assert_eq!(engine.device(), "remote:cuda:0");
    assert!(!engine.gpu_residency().resident);
    let result = engine
        .transcribe_window(
            &vec![0; 16000],
            "en",
            None,
            98,
            &OperationCancellation::default(),
            48000,
        )
        .unwrap();
    assert_eq!(result.segments[0].start_sample, 48000);
    assert_eq!(result.segments[0].end_sample, 64000);
    assert!(server.requests.lock().unwrap()[2]
        .path
        .contains("timestamps=segments"));
}

#[test]
fn real_http_errors_keep_codes_and_never_echo_untrusted_sources() {
    let server = Fixture::new();
    let client = server.client(Arc::new(crate::NoopActivity));
    for (status, expected, code) in [
        (401, RemoteErrorKind::Unauthorized, ErrorCode::Unauthorized),
        (429, RemoteErrorKind::Busy, ErrorCode::Busy),
        (
            503,
            RemoteErrorKind::ModelNotReady,
            ErrorCode::ModelNotReady,
        ),
        (502, RemoteErrorKind::WorkerFailed, ErrorCode::WorkerFailed),
    ] {
        *server.mode.lock().unwrap() = Mode::Status(status);
        let error = client
            .transcribe(&[0; 160], options(), &OperationCancellation::default())
            .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        let core = CoreError::from(&error);
        assert_eq!(core.code, code);
        assert!(!serde_json::to_string(&core).unwrap().contains("PRIVATE"));
        let classified = kind(error);
        assert_eq!(classified.kind, expected);
        assert_eq!(classified.status, Some(status));
        assert_eq!(classified.retry_after_seconds, Some(3));
    }
    *server.mode.lock().unwrap() = Mode::Disconnect;
    assert_eq!(
        kind(
            client
                .transcribe(&[0; 160], options(), &OperationCancellation::default())
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Network
    );
}

#[test]
fn cancel_closes_actual_socket_holds_admission_and_allows_next_operation() {
    for mode in [Mode::Hang, Mode::SlowBody] {
        let server = Fixture::new();
        let activity = Arc::new(TestActivity::default());
        let client = server.client(activity.clone());
        *server.mode.lock().unwrap() = mode;
        let cancellation = OperationCancellation::default();
        let (c, token) = (client.clone(), cancellation.clone());
        let work = thread::spawn(move || c.transcribe(&[0; 160], options(), &token));
        server.wait_recognition(1);
        assert!(activity.is_busy());
        assert!(!client.is_idle());
        cancellation.cancel();
        assert!(matches!(work.join().unwrap(), Err(AppError::Cancelled(_))));
        wait(|| server.disconnected.load(Ordering::Acquire));
        assert!(!activity.is_busy());
        assert!(client.is_idle());
        *server.mode.lock().unwrap() = Mode::Ok;
        assert_eq!(
            client
                .transcribe(&[0; 160], options(), &OperationCancellation::default())
                .unwrap()
                .text,
            "fixture transcript"
        );
    }
}

#[test]
fn deadline_covers_headers_and_body_and_closes_the_socket() {
    for mode in [Mode::Hang, Mode::SlowBody] {
        let server = Fixture::new();
        let activity = Arc::new(TestActivity::default());
        let endpoint = RemoteEndpoint::new(&server.endpoint, TOKEN)
            .unwrap()
            .with_timeouts(RemoteTimeouts {
                response_base: Duration::from_millis(120),
                audio_multiplier: 0.0,
                ..Default::default()
            })
            .unwrap();
        let client = RemoteRecognizer::connect(
            endpoint,
            model(),
            activity.clone(),
            &OperationCancellation::default(),
        )
        .unwrap();
        *server.mode.lock().unwrap() = mode;
        let start = Instant::now();
        let error = client
            .transcribe(&[0; 160], options(), &OperationCancellation::default())
            .unwrap_err();
        assert_eq!(kind(error).kind, RemoteErrorKind::Timeout);
        assert!(start.elapsed() < Duration::from_secs(2));
        wait(|| server.disconnected.load(Ordering::Acquire));
        assert!(!activity.is_busy());
    }
}

#[test]
fn bounded_admission_rejects_a_third_inflight_request_without_sending_audio() {
    let server = Fixture::new();
    let client = server.client(Arc::new(crate::NoopActivity));
    *server.mode.lock().unwrap() = Mode::Hang;
    let cancel = OperationCancellation::default();
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let c = client.clone();
            let token = cancel.clone();
            thread::spawn(move || c.transcribe(&[0; 160], options(), &token))
        })
        .collect();
    server.wait_recognition(2);
    assert_eq!(
        kind(
            client
                .transcribe(&[0; 160], options(), &OperationCancellation::default())
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Busy
    );
    assert_eq!(server.recognition_count(), 2);
    cancel.cancel();
    for job in jobs {
        assert!(matches!(job.join().unwrap(), Err(AppError::Cancelled(_))));
    }
}

#[test]
fn invalid_results_and_redirects_fail_without_following_or_replaying_audio() {
    let server = Fixture::new();
    let target = Fixture::new();
    let client = server.client(Arc::new(crate::NoopActivity));
    let mut wrong_model = reply(160);
    wrong_model["model"] = json!("whisper:other");
    let mut wrong_clock = reply(160);
    wrong_clock["timings"]["audio_ms"] = json!(10000);
    let mut wrong_segment = reply(160);
    wrong_segment["segments"][0]["end_ms"] = json!(10000);
    for response in [wrong_model, wrong_clock, wrong_segment] {
        *server.mode.lock().unwrap() = Mode::Reply(response);
        assert_eq!(
            kind(
                client
                    .transcribe(&[0; 160], options(), &OperationCancellation::default())
                    .unwrap_err()
            )
            .kind,
            RemoteErrorKind::InvalidResponse
        );
    }
    *server.mode.lock().unwrap() = Mode::Redirect(target.endpoint.clone());
    assert!(client
        .transcribe(&[0; 160], options(), &OperationCancellation::default())
        .is_err());
    assert!(target.requests.lock().unwrap().is_empty());
}

#[test]
fn lease_survives_until_actual_cancelled_future_cleanup_finishes() {
    struct Cleanup {
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.entered.store(true, Ordering::Release);
            wait(|| self.release.load(Ordering::Acquire));
        }
    }
    let activity = Arc::new(TestActivity::default());
    let transport = Arc::new(
        transport::Transport::new(
            RemoteEndpoint::new("http://127.0.0.1:1", TOKEN).unwrap(),
            activity.clone(),
        )
        .unwrap(),
    );
    let cancellation = OperationCancellation::default();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let cleanup = Cleanup {
        entered: entered.clone(),
        release: release.clone(),
    };
    let (http, cancel) = (transport.clone(), cancellation.clone());
    let job = thread::spawn(move || {
        http.call(&cancel, Duration::from_secs(10), || {
            Ok(async move {
                let _cleanup = cleanup;
                std::future::pending::<AppResult<()>>().await
            })
        })
    });
    wait(|| !transport.is_idle());
    cancellation.cancel();
    wait(|| entered.load(Ordering::Acquire));
    assert!(activity.is_busy());
    assert!(!transport.is_idle());
    assert!(!job.is_finished());
    release.store(true, Ordering::Release);
    assert!(matches!(job.join().unwrap(), Err(AppError::Cancelled(_))));
    assert!(!activity.is_busy());
    assert!(transport.is_idle());
}

#[test]
fn unsupported_options_and_pre_cancelled_audio_are_never_transmitted() {
    let server = Fixture::new();
    let client = server.client(Arc::new(crate::NoopActivity));
    let cancel = OperationCancellation::default();
    cancel.cancel();
    assert!(matches!(
        client.transcribe(&[0; 160], options(), &cancel),
        Err(AppError::Cancelled(_))
    ));
    assert!(client
        .transcribe(
            &[0; 160],
            RecognitionOptions {
                language: "xx",
                ..options()
            },
            &OperationCancellation::default()
        )
        .is_err());
    assert!(client
        .transcribe_window(
            &[0; 160],
            WindowOptions {
                recognition: options(),
                context: Some("PRIVATE context"),
                operation_id: 1,
                audio_start_sample: 0
            },
            &OperationCancellation::default()
        )
        .is_err());
    assert_eq!(server.recognition_count(), 0);
}

#[test]
fn health_probe_is_authenticated_metadata_only_and_preserves_recognizer_state() {
    let server = Fixture::new();
    let activity = Arc::new(TestActivity::default());
    let result = probe_health(
        server.endpoint(),
        &model(),
        activity.clone(),
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(
        result,
        RemoteHealth {
            server: RemoteServerHealth::Ready,
            selected_model: RemoteModelHealth::Ready,
            interactive_queue: 2,
            batch_queue: 3
        }
    );
    assert!(!activity.is_busy());
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("PRIVATE"));
    assert!(!encoded.contains("whisper"));
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/health");
        assert!(requests[0].headers.contains(&format!("Bearer {TOKEN}")));
        assert!(requests[0].body.is_empty());
    }
    let client = server.client(activity);
    assert_eq!(
        client
            .probe_health(&OperationCancellation::default())
            .unwrap()
            .server,
        RemoteServerHealth::Ready
    );
    assert_eq!(client.device_name(), "remote:cuda:0");
    assert!(client.is_idle());
    assert_eq!(server.recognition_count(), 0);
}

#[test]
fn health_probe_accepts_starting_or_failed_503_and_unknown_on_demand_model() {
    let server = Fixture::new();
    for (status, state) in [
        ("starting", RemoteServerHealth::Starting),
        ("failed", RemoteServerHealth::Failed),
        ("stopping", RemoteServerHealth::Stopping),
    ] {
        *server.mode.lock().unwrap() = Mode::HealthReply(503, health(status, "loading"));
        let value = probe_health(
            server.endpoint(),
            &model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap();
        assert_eq!(value.server, state);
        assert_eq!(value.selected_model, RemoteModelHealth::Loading);
    }
    let mut document = health("ready", "ready");
    document["models"][0]["id"] = json!("whisper:different-preload");
    *server.mode.lock().unwrap() = Mode::HealthReply(200, document);
    let value = probe_health(
        server.endpoint(),
        &model(),
        Arc::new(crate::NoopActivity),
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(value.server, RemoteServerHealth::Ready);
    assert_eq!(value.selected_model, RemoteModelHealth::Unknown);
}

#[test]
fn health_probe_rejects_malformed_contradictory_and_unbounded_metadata() {
    let server = Fixture::new();
    let mut duplicate = health("ready", "ready");
    let first = duplicate["models"][0].clone();
    duplicate["models"].as_array_mut().unwrap().push(first);
    let mut unknown = health("ready", "PRIVATE");
    unknown["status"] = json!("PRIVATE");
    let mut huge_queue = health("ready", "ready");
    huge_queue["queue"]["batch"] = json!(4294967296_u64);
    for (status, document) in [
        (200, json!({"error":{"message":"PRIVATE"}})),
        (200, duplicate),
        (200, unknown),
        (200, huge_queue),
        (200, health("failed", "failed")),
        (503, health("ready", "ready")),
    ] {
        *server.mode.lock().unwrap() = Mode::HealthReply(status, document);
        let error = probe_health(
            server.endpoint(),
            &model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        assert_eq!(kind(error).kind, RemoteErrorKind::InvalidResponse);
    }
    *server.mode.lock().unwrap() = Mode::Oversized;
    assert_eq!(
        kind(
            probe_health(
                server.endpoint(),
                &model(),
                Arc::new(crate::NoopActivity),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
}

#[test]
fn health_probe_classifies_authentication_and_disconnect_without_echoing_errors() {
    let server = Fixture::new();
    for (mode, expected) in [
        (Mode::Status(401), RemoteErrorKind::Unauthorized),
        (Mode::Disconnect, RemoteErrorKind::Network),
    ] {
        *server.mode.lock().unwrap() = mode;
        let error = probe_health(
            server.endpoint(),
            &model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        assert_eq!(kind(error).kind, expected);
    }
    let cancel = OperationCancellation::default();
    cancel.cancel();
    let requests = server.requests.lock().unwrap().len();
    assert!(matches!(
        probe_health(
            server.endpoint(),
            &model(),
            Arc::new(crate::NoopActivity),
            &cancel
        ),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(server.requests.lock().unwrap().len(), requests);
}

#[test]
fn health_probe_deadline_covers_headers_and_body_then_releases_admission() {
    for mode in [Mode::Hang, Mode::SlowBody] {
        let server = Fixture::new();
        let activity = Arc::new(TestActivity::default());
        *server.mode.lock().unwrap() = mode;
        let started = Instant::now();
        let error = probe_health(
            server.endpoint(),
            &model(),
            activity.clone(),
            &OperationCancellation::default(),
        )
        .unwrap_err();
        assert_eq!(kind(error).kind, RemoteErrorKind::Timeout);
        assert!(started.elapsed() < Duration::from_secs(2));
        wait(|| server.disconnected.load(Ordering::Acquire));
        assert!(!activity.is_busy());
        assert_eq!(server.recognition_count(), 0);
    }
}

#[test]
fn health_probe_cancellation_closes_socket_and_allows_another_probe() {
    let server = Fixture::new();
    let activity = Arc::new(TestActivity::default());
    *server.mode.lock().unwrap() = Mode::Hang;
    let endpoint = server.endpoint();
    let token = OperationCancellation::default();
    let cancellation = token.clone();
    let owned_activity = activity.clone();
    let job =
        thread::spawn(move || probe_health(endpoint, &model(), owned_activity, &cancellation));
    wait(|| !server.requests.lock().unwrap().is_empty());
    assert!(activity.is_busy());
    token.cancel();
    assert!(matches!(job.join().unwrap(), Err(AppError::Cancelled(_))));
    wait(|| server.disconnected.load(Ordering::Acquire));
    assert!(!activity.is_busy());
    *server.mode.lock().unwrap() = Mode::Ok;
    assert_eq!(
        probe_health(
            server.endpoint(),
            &model(),
            activity,
            &OperationCancellation::default()
        )
        .unwrap()
        .server,
        RemoteServerHealth::Ready
    );
}
