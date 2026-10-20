use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use fono_core::{OperationCoordinator, OperationSource};

#[path = "worker_cleanup_tests.rs"]
mod cleanup;

// Parallel workspace tests can briefly delay Windows cmd fixture startup;
// keep this distinct from the production request deadlines.
const TEST_TIMEOUT: Duration = Duration::from_secs(1);
const TEST_TIMEOUTS: WorkerTimeouts = WorkerTimeouts {
    ping: TEST_TIMEOUT,
    load: TEST_TIMEOUT,
    transcribe_base: TEST_TIMEOUT,
    transcribe_max: TEST_TIMEOUT,
};
// EOF tests verify transport semantics, not inference deadlines. Their file
// barrier/controller needs scheduling headroom during parallel workspace gates.
const EOF_FIXTURE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(windows)]
#[test]
fn common_recognizer_preserves_worker_model_device_and_negotiated_capabilities() {
    let fixture = WorkerFixture::create("echo {\"type\":\"result\",\"protocol_version\":4,\"request_id\":\"transcribe-3\",\"operation_id\":\"dictation-transcribe-3\",\"text\":\"contract result\",\"audio_secs\":0.01,\"transcribe_secs\":0.02,\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang");
    let model_path = fixture.directory.join("ggml-small.bin");
    fs::write(&model_path, b"protocol fixture, not a neural model").unwrap();
    let model = SttModelSelection::from_whisper_path(model_path.to_string_lossy().into_owned());
    let engine = crate::stt::SttEngine::new();
    let paths = crate::stt::WorkerPaths::for_test_workers(Some(fixture.script.clone()), None);
    engine
        .ensure_model_loaded(
            &model,
            crate::types::AccelerationMode::Cuda,
            &paths,
            &OperationCancellation::default(),
        )
        .unwrap();
    assert_eq!(engine.model(), Some(model.id));
    assert_eq!(engine.device(), "CUDA");
    let capabilities = engine.capabilities().unwrap();
    assert!(capabilities.window_transcription && capabilities.token_timestamps);
    assert!(!capabilities.decoding_options);
    assert!(matches!(
        engine.transcribe_with_options(
            &[0; 160],
            "ru",
            fono_stt_protocol::DecodingOptions {
                strategy: fono_stt_protocol::DecodingStrategy::Accurate,
                threads: None,
            }
        ),
        Err(AppError::Config(_))
    ));
    let transcript = engine.transcribe(&[0; 160], "ru").unwrap();
    assert_eq!(transcript.text, "contract result");
    assert_eq!(transcript.device.as_deref(), Some("CUDA"));
    assert_eq!(transcript.audio_secs, Some(0.01));
}

#[cfg(windows)]
#[test]
fn old_worker_rejects_nondefault_decoding_before_consuming_a_request() {
    let fixture = WorkerFixture::create("echo {\"type\":\"pong\",\"protocol_version\":4,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang");
    let mut session = fixture.start();
    let error = session
        .transcribe_cancellable_with_options(
            &[0; 160],
            "auto",
            &OperationCancellation::default(),
            fono_stt_protocol::DecodingOptions {
                strategy: fono_stt_protocol::DecodingStrategy::Accurate,
                threads: None,
            },
            &[],
        )
        .unwrap_err();
    assert!(matches!(error, AppError::Config(_)));
    assert_eq!(session.next_request_id, 2);
    session.ping().unwrap();
    assert!(session.stdin_tx.is_some());
}

struct WorkerFixture {
    directory: PathBuf,
    script: PathBuf,
}

impl WorkerFixture {
    fn create(after_load: &str) -> Self {
        // A fake nonresponding worker should sleep on stdin rather than burn
        // a CPU core in a cmd goto loop and starve other protocol fixtures.
        let after_load = after_load.replace(
            ":hang\r\ngoto hang",
            ":hang\r\nset /p ignored=\r\ngoto hang",
        );
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fono-stt-worker-fixture-{}-{unique}",
            process::id()
        ));
        fs::create_dir(&directory).expect("create worker fixture directory");
        let script = directory.join("worker.cmd");
        fs::write(
                &script,
                format!(
                    "@echo off\r\nset /p request=\r\necho {{\"type\":\"ready\",\"protocol_version\":4,\"request_id\":\"hello-1\",\"backend\":\"cuda\",\"capabilities\":{{\"protocol_version\":4,\"supports_health\":true,\"supports_shutdown\":true,\"supports_window\":true,\"supports_cancel\":true,\"supports_token_timestamps\":true,\"maximum_request_bytes\":16777216,\"maximum_response_bytes\":1048576}}}}\r\nset /p request=\r\necho {{\"type\":\"model_loaded\",\"protocol_version\":4,\"request_id\":\"load-2\",\"backend\":\"cuda\"}}\r\nset /p request=\r\n{after_load}\r\n"
                ),
            )
            .expect("write worker fixture");
        Self { directory, script }
    }

    #[cfg(windows)]
    fn request_dispatcher() -> Self {
        let fixture = Self::create("");
        let dispatcher = fixture.directory.join("worker.ps1");
        // This fixture needs a real line-framed request loop: a fixed third
        // set /p can mistake a partial/empty pipe read for Shutdown and emit an
        // unsolicited ACK/EOF before model readiness has been published.
        fs::write(
            &dispatcher,
            r#"$ErrorActionPreference = 'Stop'
while ($null -ne ($line = [Console]::In.ReadLine())) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $request = ConvertFrom-Json -InputObject $line
    if ($request.protocol_version -ne 4 -or
        [string]::IsNullOrWhiteSpace([string]$request.request_id)) {
        throw 'Invalid fixture request metadata'
    }
    $response = [ordered]@{
        type = ''
        protocol_version = 4
        request_id = [string]$request.request_id
        backend = 'cuda'
    }
    switch ($request.type) {
        'hello' {
            $response['type'] = 'ready'
            $response['capabilities'] = @{
                protocol_version = 4
                supports_health = $true
                supports_shutdown = $true
                supports_window = $true
                supports_cancel = $true
                supports_token_timestamps = $true
                maximum_request_bytes = 16777216
                maximum_response_bytes = 1048576
            }
        }
        'load' { $response['type'] = 'model_loaded' }
        'ping' { $response['type'] = 'pong' }
        'shutdown' { $response['type'] = 'shutting_down' }
        default { throw 'Unexpected fixture request type' }
    }
    [Console]::Out.WriteLine(($response | ConvertTo-Json -Depth 4 -Compress))
    [Console]::Out.Flush()
    if ($request.type -eq 'shutdown') { exit 0 }
}
"#,
        )
        .expect("write request-dispatch fixture");
        fs::write(
            &fixture.script,
            "@echo off\r\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0worker.ps1\"\r\nexit /b %errorlevel%\r\n",
        )
        .expect("write request-dispatch wrapper");
        fixture
    }

    fn start(&self) -> WorkerSession {
        WorkerSession::start_with_timeouts(
            self.script.clone(),
            BackendKind::Cuda,
            Path::new("fixture-model.bin"),
            TEST_TIMEOUTS,
        )
        .expect("fixture worker completes handshake")
    }
}

impl Drop for WorkerFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn assert_session_is_terminated(session: &mut WorkerSession) {
    // Request failure may transfer process/IO ownership to the cleanup
    // supervisor. Assert confirmed cleanup, not an immediate post-error exit.
    let deadline = Instant::now() + EOF_FIXTURE_TIMEOUT;
    while session.cleanup.is_pending() {
        assert!(Instant::now() < deadline, "fixture cleanup did not finish");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(session.child.try_wait(), Ok(Some(_))));
    assert!(session.stdin_tx.is_none());
    assert!(session.stdin_thread.is_none());
    assert!(session.stdout_thread.is_none());
    assert!(session.stderr_thread.is_none());
}

#[test]
fn package_probe_requires_strict_hello_without_loading_a_model() {
    let fixture = WorkerFixture::create("");
    let ready = "{\"type\":\"ready\",\"protocol_version\":4,\"request_id\":\"hello-1\",\"backend\":\"cuda\",\"capabilities\":{\"protocol_version\":4,\"supports_health\":true,\"supports_shutdown\":true,\"supports_window\":true,\"supports_cancel\":true,\"supports_token_timestamps\":true,\"maximum_request_bytes\":16777216,\"maximum_response_bytes\":1048576}}";
    let script = format!(
        "@echo off\r\nset /p request=\r\necho {ready}\r\nset /p request=\r\necho {{\"type\":\"shutting_down\",\"protocol_version\":4,\"request_id\":\"shutdown-2\",\"backend\":\"cuda\"}}\r\nexit /b 0\r\n"
    );
    fs::write(&fixture.script, &script).unwrap();
    WorkerSession::probe(fixture.script.clone(), None)
        .expect("probe sends hello then shutdown, never load");
    fs::write(
        &fixture.script,
        script.replace("\"supports_cancel\":true", "\"supports_cancel\":false"),
    )
    .unwrap();
    assert!(WorkerSession::probe(fixture.script.clone(), None).is_err());
}

#[test]
fn rejected_uncommitted_runtime_retires_worker_and_publishes_failed_readiness() {
    let fixture = WorkerFixture::create(":hang\r\nset /p ignored=\r\ngoto hang");
    let engine = crate::stt::SttEngine::new();
    *engine.state.lock() = crate::stt::routing::EngineState::Worker {
        session: Arc::new(WorkerMailbox::start(fixture.start())),
        device: "CUDA",
    };
    let generation = engine.generation.load(std::sync::atomic::Ordering::Acquire);
    engine.reject_prepared_selection("installation rolled back");
    assert!(matches!(
        engine.readiness(),
        crate::stt::SttReadiness::Failed { .. }
    ));
    assert!(!engine.is_loaded());
    assert!(engine.generation.load(std::sync::atomic::Ordering::Acquire) > generation);
}

#[test]
fn response_reader_enforces_limit_and_recovers_at_next_line() {
    let mut input = Cursor::new(b"response-too-long\nok\n");

    let error = read_limited_line(&mut input, 8).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
    assert_eq!(read_limited_line(&mut input, 8).unwrap(), Some("ok".into()));
    assert_eq!(read_limited_line(&mut input, 8).unwrap(), None);
}

#[test]
fn transcription_timeout_is_bounded() {
    assert_eq!(transcribe_timeout(0), TRANSCRIBE_BASE_TIMEOUT);
    assert_eq!(transcribe_timeout(usize::MAX), TRANSCRIBE_MAX_TIMEOUT);
}

#[test]
fn base64_transport_measurement_for_typical_recording_lengths() {
    const SAMPLE_RATE: usize = 16_000;
    for seconds in [5_usize, 30, 120] {
        let samples = vec![123_i16; seconds * SAMPLE_RATE];
        let started = Instant::now();
        let encoded = encode_samples_i16_base64(&samples);
        let elapsed = started.elapsed();
        let request = WorkerRequest::Transcribe {
            meta: RequestMeta::new("measurement", Some("measurement-operation".into())),
            model_path: "model.bin".into(),
            language: "auto".into(),
            glossary: Vec::new(),
            samples_i16_base64: encoded,
            decoding: fono_stt_protocol::DecodingOptions::default(),
        };
        let json = serde_json::to_string(&request).expect("serialize measurement request");
        println!(
            "stt transport: {seconds}s pcm={}B json={}B encode={}ms",
            std::mem::size_of_val(samples.as_slice()),
            json.len(),
            elapsed.as_millis()
        );
        assert!(json.len() <= MAX_REQUEST_FRAME_BYTES);
    }
}

#[test]
fn maximum_recording_fits_the_versioned_transport_frame() {
    const SAMPLE_RATE: usize = 16_000;
    const MAX_RECORDING_SECONDS: usize = 5 * 60;
    let samples = vec![0_i16; MAX_RECORDING_SECONDS * SAMPLE_RATE];
    let encoded = encode_samples_i16_base64(&samples);
    let request = WorkerRequest::Transcribe {
        meta: RequestMeta::new("maximum", Some("maximum-operation".into())),
        model_path: "model.bin".into(),
        language: "auto".into(),
        glossary: Vec::new(),
        samples_i16_base64: encoded,
        decoding: fono_stt_protocol::DecodingOptions::default(),
    };
    let json = serde_json::to_string(&request).expect("serialize maximum request");
    assert!(json.len() <= MAX_REQUEST_FRAME_BYTES);
}

#[cfg(windows)]
#[test]
fn malformed_worker_response_terminates_the_session() {
    let fixture = WorkerFixture::create("echo not-json\r\n:hang\r\ngoto hang");
    let mut session = fixture.start();

    let error = session.transcribe(&[0; 160], "auto").unwrap_err();

    assert!(error.to_string().contains("invalid JSON"));
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn worker_exit_before_response_terminates_the_session() {
    // cmd's set /p can return before the writer's separate newline syscall.
    // Keep stdin open until a FIFO writer barrier acknowledges the complete
    // frame; otherwise this fixture can exercise broken pipe instead of EOF.
    let fixture = WorkerFixture::create(
        ">\"%~dp0request-observed\" echo ready\r\n:await_exit\r\nif not exist \"%~dp0allow-exit\" goto await_exit\r\nexit /b 17",
    );
    let mut session = fixture.start();
    session.timeouts.transcribe_base = EOF_FIXTURE_TIMEOUT;
    session.timeouts.transcribe_max = EOF_FIXTURE_TIMEOUT;
    let writer = session
        .stdin_tx
        .as_ref()
        .expect("open worker stdin")
        .clone();
    let observed = fixture.directory.join("request-observed");
    let release = fixture.directory.join("allow-exit");
    let release_worker = thread::spawn(move || {
        let deadline = Instant::now() + EOF_FIXTURE_TIMEOUT;
        while !observed.exists() {
            assert!(
                Instant::now() < deadline,
                "fixture observes transcription request"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let (result_tx, result_rx) = bounded(1);
        writer
            .send_timeout(
                WorkerWrite {
                    line: String::new(),
                    result_tx,
                },
                remaining_until(deadline),
            )
            .expect("enqueue barrier after transcription frame");
        result_rx
            .recv_timeout(remaining_until(deadline))
            .expect("writer acknowledges FIFO barrier")
            .expect("complete request reached open worker stdin");
        fs::write(release, b"exit").expect("release worker only after completed write");
    });

    let error = session.transcribe(&[0; 160], "auto").unwrap_err();
    release_worker.join().expect("release controller completes");

    assert!(
        error
            .to_string()
            .contains("exited before returning a response"),
        "expected response EOF after worker exit, got: {error}"
    );
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn foreign_request_id_terminates_the_session() {
    let fixture = WorkerFixture::create(
            "echo {\"type\":\"result\",\"protocol_version\":4,\"request_id\":\"foreign\",\"operation_id\":\"dictation-transcribe-3\",\"text\":\"x\",\"audio_secs\":0.01,\"transcribe_secs\":0.01,\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
        );
    let mut session = fixture.start();

    let error = session.transcribe(&[0; 160], "auto").unwrap_err();

    assert!(error.to_string().contains("returned request_id foreign"));
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn hung_worker_response_respects_deadline_and_terminates_the_session() {
    let fixture = WorkerFixture::create(":hang\r\ngoto hang");
    let mut session = fixture.start();
    let started = Instant::now();

    let error = session.transcribe(&[0; 160], "auto").unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(error.to_string().contains("timed out"));
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn cancelled_transcription_terminates_hung_worker() {
    let fixture = WorkerFixture::create(":hang\r\ngoto hang");
    let mut session = fixture.start();
    let coordinator = Arc::new(OperationCoordinator::new());
    let operation = coordinator
        .start(OperationSource::Ui)
        .expect("start operation for cancellation test");
    let cancellation = coordinator
        .cancellation(operation.id)
        .expect("cancellation signal belongs to the active operation");
    let coordinator_for_cancel = Arc::clone(&coordinator);
    let cancel = thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        coordinator_for_cancel.cancel(operation.id);
    });
    let started = Instant::now();

    let error = session
        .transcribe_cancellable(&[0; 160], "auto", &cancellation)
        .expect_err("cancelled transcription cannot wait for the worker deadline");

    cancel.join().expect("cancellation thread completes");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(error, AppError::Cancelled(_)));
    assert!(error.to_string().contains("worker request cancelled"));
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn mailbox_health_uses_the_owner_thread_protocol_session() {
    let fixture = WorkerFixture::create(
            "echo {\"type\":\"pong\",\"protocol_version\":4,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
        );
    let mailbox = WorkerMailbox::start(fixture.start());

    assert!(matches!(mailbox.health(), MailboxHealth::Ready));
    assert!(mailbox.alive.load(Ordering::Acquire));
}

#[cfg(windows)]
#[test]
fn mailbox_cancellation_stops_a_hung_owner_session() {
    let fixture = WorkerFixture::create(":hang\r\ngoto hang");
    let mailbox = WorkerMailbox::start(fixture.start());
    let (fault_tx, fault_rx) = bounded(1);
    mailbox.arm_fault(Arc::new(move || {
        let _ = fault_tx.try_send(());
    }));
    let coordinator = Arc::new(OperationCoordinator::new());
    let operation = coordinator.start(OperationSource::Ui).unwrap();
    let cancellation = coordinator.cancellation(operation.id).unwrap();
    let coordinator_for_cancel = Arc::clone(&coordinator);
    let cancel = thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        coordinator_for_cancel.cancel(operation.id);
    });

    let error = mailbox
        .transcribe_cancellable(&[0; 160], "auto", &cancellation)
        .expect_err("cancelled mailbox request cannot wait for worker deadline");

    cancel.join().unwrap();
    assert!(matches!(error, AppError::Cancelled(_)));
    for _ in 0..20 {
        if !mailbox.alive.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!mailbox.alive.load(Ordering::Acquire));
    assert!(fault_rx.recv_timeout(Duration::from_millis(50)).is_err());
}

#[cfg(windows)]
fn recovery_fixture(
    after_load: &str,
) -> (
    WorkerFixture,
    Arc<crate::stt::SttEngine>,
    Arc<Mutex<Option<crate::stt::RecoverySelection>>>,
) {
    recovery_fixture_with_worker(WorkerFixture::create(after_load))
}

#[cfg(windows)]
fn recovery_fixture_with_worker(
    fixture: WorkerFixture,
) -> (
    WorkerFixture,
    Arc<crate::stt::SttEngine>,
    Arc<Mutex<Option<crate::stt::RecoverySelection>>>,
) {
    let model_path = fixture.directory.join("fixture-model.bin");
    fs::write(&model_path, []).unwrap();
    let selection = crate::stt::RecoverySelection {
        model_path: model_path.clone(),
        acceleration: crate::types::AccelerationMode::Cuda,
        worker_paths: crate::stt::WorkerPaths::for_test_workers(Some(fixture.script.clone()), None),
    };
    let provider = Arc::new(Mutex::new(Some(selection.clone())));
    let selected = provider.clone();
    let engine = Arc::new(crate::stt::SttEngine::new());
    engine
        .enable_recovery_with_timing(
            Arc::new(move || selected.lock().clone()),
            crate::stt::recovery::Timing {
                backoff: [
                    Duration::from_millis(30),
                    Duration::from_millis(60),
                    Duration::from_millis(100),
                ],
                stable_window: Duration::from_secs(60),
            },
        )
        .unwrap();
    engine
        .ensure_loaded(
            &selection.model_path,
            selection.acceleration,
            &selection.worker_paths,
        )
        .unwrap();
    (fixture, engine, provider)
}

#[cfg(windows)]
fn kill_only_fixture_worker(engine: &crate::stt::SttEngine) {
    let child = match &*engine.state.lock() {
        crate::stt::EngineState::Worker { session, .. } => session.child.clone(),
        _ => panic!("fake worker is not installed"),
    };
    child.kill().unwrap();
}

#[cfg(windows)]
fn wait_for_fixture_state(check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(
            Instant::now() < deadline,
            "fake worker state did not settle"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(windows)]
#[test]
fn idle_fixture_worker_eof_recovers_without_health_or_transcription_poll() {
    let (_fixture, engine, _provider) = recovery_fixture(":hang\r\ngoto hang");
    let old = engine.generation.load(Ordering::Acquire);
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(|| {
        engine.generation.load(Ordering::Acquire) > old
            && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
    });
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn repeated_fixture_worker_crashes_stop_after_three_automatic_reloads() {
    let (_fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    for _ in 0..3 {
        let old = engine.generation.load(Ordering::Acquire);
        kill_only_fixture_worker(&engine);
        wait_for_fixture_state(|| {
            engine.generation.load(Ordering::Acquire) > old
                && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
        });
    }
    let bounded = engine.generation.load(Ordering::Acquire);
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(
        || matches!(engine.readiness(), crate::stt::SttReadiness::Failed { ref message } if message.contains("трёх")),
    );
    thread::sleep(Duration::from_millis(250));
    assert_eq!(engine.generation.load(Ordering::Acquire), bounded);
    // The explicit UI model-selection route resets this budget and calls
    // ensure_loaded even when the selected model path has not changed.
    let selected = provider.lock().clone().unwrap();
    wait_for_fixture_state(|| match &*engine.state.lock() {
        crate::stt::EngineState::Worker { session, .. } => session.is_retired(),
        crate::stt::EngineState::Empty => true,
        _ => false,
    });
    engine.reset_recovery_budget();
    engine
        .ensure_loaded(
            &selected.model_path,
            selected.acceleration,
            &selected.worker_paths,
        )
        .unwrap();
    let manual_generation = engine.generation.load(Ordering::Acquire);
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(|| {
        engine.generation.load(Ordering::Acquire) > manual_generation
            && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
    });
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn stale_fixture_fault_cannot_reload_old_model_after_new_selection() {
    let (fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    let mut next = provider.lock().clone().unwrap();
    next.model_path = fixture.directory.join("new-model.bin");
    fs::write(&next.model_path, []).unwrap();
    kill_only_fixture_worker(&engine);
    *provider.lock() = Some(next.clone());
    // Normal model selection serializes against recovery on the same load gate.
    wait_for_fixture_state(|| !engine.cleanup.is_pending());
    engine
        .ensure_loaded(&next.model_path, next.acceleration, &next.worker_paths)
        .unwrap();
    let selected_generation = engine.generation.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        engine.generation.load(Ordering::Acquire),
        selected_generation
    );
    assert!(
        matches!(&*engine.state.lock(), crate::stt::EngineState::Worker { session, .. }
        if session.model_path == next.model_path.to_string_lossy())
    );
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn removed_selection_and_shutdown_cancel_queued_fixture_recovery() {
    for shutdown in [false, true] {
        let (_fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
        let old = engine.generation.load(Ordering::Acquire);
        kill_only_fixture_worker(&engine);
        if shutdown {
            engine.stop_recovery();
        } else {
            *provider.lock() = None;
        }
        thread::sleep(Duration::from_millis(250));
        assert_eq!(engine.generation.load(Ordering::Acquire), old);
        engine.stop_recovery();
    }
}

#[cfg(windows)]
#[test]
fn intentional_adaptive_release_does_not_trigger_fixture_recovery() {
    let (_fixture, engine, _provider) =
        recovery_fixture_with_worker(WorkerFixture::request_dispatcher());
    // A health request is legal before adaptive release. Its response must not
    // consume the fixture's eventual Shutdown or depend on a fixed request ID.
    wait_for_fixture_state(|| match engine.health() {
        crate::stt::SttHealth::Ready { .. } => true,
        crate::stt::SttHealth::Busy { .. } => false,
        other => panic!("request-dispatch fixture health failed: {other:?}"),
    });
    let old = engine.generation.load(Ordering::Acquire);
    assert!(engine.release_idle_gpu(old, |detach| detach()));
    let released_generation = engine.generation.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        engine.generation.load(Ordering::Acquire),
        released_generation
    );
    assert!(matches!(
        &*engine.state.lock(),
        crate::stt::EngineState::Empty
    ));
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn stale_health_failure_cannot_overwrite_replacement_ready() {
    let (fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    let old = engine.state.lock().active().unwrap();
    // The probe has retained this old identity; native cleanup can finish
    // before its delayed error is delivered. Keep recovery out of this setup
    // so only the explicit replacement publishes Ready.
    engine.stop_recovery();
    if let crate::stt::ActiveEngine::Worker(session) = &old {
        session.shutdown();
    }
    wait_for_fixture_state(|| !engine.cleanup.is_pending());
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    engine.set_readiness_observer(Arc::new(move |event| observed.lock().push(event)));
    let mut next = provider.lock().clone().unwrap();
    next.model_path = fixture.directory.join("replacement.bin");
    fs::write(&next.model_path, []).unwrap();
    *provider.lock() = Some(next.clone());
    engine
        .ensure_loaded(&next.model_path, next.acceleration, &next.worker_paths)
        .unwrap();
    assert!(matches!(
        engine.readiness(),
        crate::stt::SttReadiness::Ready { .. }
    ));
    // Deterministically deliver the old probe's failure after replacement Ready.
    assert!(!engine.retire_failed_active(&old, "late old health failure".into()));
    assert!(matches!(
        engine.readiness(),
        crate::stt::SttReadiness::Ready { .. }
    ));
    assert!(matches!(
        events.lock().last(),
        Some(crate::stt::SttReadiness::Ready { .. })
    ));
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn health_fault_during_publication_is_armed_before_retirement_and_recovers() {
    let (fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    let mut next = provider.lock().clone().unwrap();
    next.model_path = fixture.directory.join("publication-model.bin");
    fs::write(&next.model_path, []).unwrap();
    *provider.lock() = Some(next.clone());
    let (entered_tx, entered_rx) = bounded(1);
    let (continue_tx, continue_rx) = bounded(1);
    *engine.before_recovery_arm.lock() = Some(Arc::new(move || {
        entered_tx.send(()).unwrap();
        continue_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let loader_engine = engine.clone();
    let loader = thread::spawn(move || {
        loader_engine.ensure_loaded(&next.model_path, next.acceleration, &next.worker_paths)
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let failed = engine.state.lock().active().unwrap();
    let selected_generation = engine.generation.load(Ordering::Acquire);
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(|| failed.worker_faulted());
    let retiring_engine = engine.clone();
    let (retired_tx, retired_rx) = bounded(1);
    let retirement = thread::spawn(move || {
        let retired =
            retiring_engine.retire_failed_active(&failed, "fixture health failure".into());
        retired_tx.send(retired).unwrap();
    });
    // Publication must exclude retirement until the fault observer is armed.
    let early_retirement = retired_rx.recv_timeout(Duration::from_millis(50));
    continue_tx.send(()).unwrap();
    let _ = loader.join().unwrap();
    retirement.join().unwrap();
    assert!(
        early_retirement.is_err(),
        "health retired an unarmed mailbox"
    );
    wait_for_fixture_state(|| {
        engine.generation.load(Ordering::Acquire) > selected_generation
            && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
    });
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn persistence_ack_requeues_only_the_exact_current_fault() {
    let (fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    let old_selection = provider.lock().clone().unwrap();
    let old_token = engine.prepared_recovery_selection(&old_selection).unwrap();
    *provider.lock() = None;
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(|| match &*engine.state.lock() {
        crate::stt::EngineState::Worker { session, .. } => session.is_retired(),
        crate::stt::EngineState::Empty => true,
        _ => false,
    });
    let mut next = old_selection;
    next.model_path = fixture.directory.join("prepared-not-persisted.bin");
    fs::write(&next.model_path, []).unwrap();
    engine.reset_recovery_budget();
    engine
        .ensure_loaded(&next.model_path, next.acceleration, &next.worker_paths)
        .unwrap();
    let token = engine.prepared_recovery_selection(&next).unwrap();
    let prepared_generation = engine.generation.load(Ordering::Acquire);
    kill_only_fixture_worker(&engine);
    wait_for_fixture_state(|| match &*engine.state.lock() {
        crate::stt::EngineState::Worker { session, .. } => session.is_retired(),
        crate::stt::EngineState::Empty => true,
        _ => false,
    });
    // A failed/uncommitted settings transaction never acknowledges its prepare.
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        engine.generation.load(Ordering::Acquire),
        prepared_generation
    );
    assert!(!engine.acknowledge_recovery_selection(old_token));
    assert_eq!(
        engine.generation.load(Ordering::Acquire),
        prepared_generation
    );
    // Successful persistence publishes the exact selection, then acknowledges
    // its generation. The already-emitted/discarded fault must be recoverable.
    *provider.lock() = Some(next.clone());
    assert!(engine.acknowledge_recovery_selection(token.clone()));
    wait_for_fixture_state(|| {
        engine.generation.load(Ordering::Acquire) > prepared_generation
            && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
    });
    assert!(!engine.acknowledge_recovery_selection(token));
    assert!(
        matches!(&*engine.state.lock(), crate::stt::EngineState::Worker { session, .. }
        if session.model_path == next.model_path.to_string_lossy())
    );
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn explicit_acceleration_change_replaces_compatible_worker_and_recovers_saved_selection() {
    // This fixture has no request after Load until explicit cleanup. Do not
    // model Shutdown with a cmd `set /p` counter: an independently delivered
    // trailing newline could advance that counter without a third JSON frame.
    let (_fixture, engine, provider) = recovery_fixture(":hang\r\ngoto hang");
    let mut selected = provider.lock().clone().unwrap();
    for acceleration in [
        crate::types::AccelerationMode::Auto,
        crate::types::AccelerationMode::Cuda,
    ] {
        let old_generation = engine.generation.load(Ordering::Acquire);
        engine
            .ensure_loaded(
                &selected.model_path,
                selected.acceleration,
                &selected.worker_paths,
            )
            .unwrap();
        assert_eq!(
            engine.generation.load(Ordering::Acquire),
            old_generation,
            "ordinary ensure must reuse exact selection"
        );
        // Retain the old probe's handle to exclude native teardown from the
        // selection-publication assertion, then finish its cleanup explicitly.
        let old = engine.state.lock().active().unwrap();
        selected.acceleration = acceleration;
        engine.reset_recovery_budget();
        engine
            .ensure_loaded(
                &selected.model_path,
                selected.acceleration,
                &selected.worker_paths,
            )
            .unwrap();
        assert!(
            !engine.state.lock().contains(&old),
            "changed acceleration reused a callback bound to old settings"
        );
        let prepared_generation = engine.generation.load(Ordering::Acquire);
        assert!(prepared_generation > old_generation);
        let prepared = engine.prepared_recovery_selection(&selected).unwrap();
        if let crate::stt::ActiveEngine::Worker(session) = &old {
            session.shutdown();
        }
        drop(old);
        wait_for_fixture_state(|| !engine.cleanup.is_pending());
        *provider.lock() = Some(selected.clone());
        assert!(engine.acknowledge_recovery_selection(prepared));
        kill_only_fixture_worker(&engine);
        wait_for_fixture_state(|| {
            engine.generation.load(Ordering::Acquire) > prepared_generation
                && matches!(engine.readiness(), crate::stt::SttReadiness::Ready { .. })
        });
    }
    engine.stop_recovery();
}

#[cfg(windows)]
#[test]
fn ordinary_wrong_operation_or_backend_retires_and_faults_the_protocol_worker() {
    for (backend, operation) in [
        ("cuda", "another-operation"),
        ("vulkan", "dictation-transcribe-3"),
    ] {
        let fixture = WorkerFixture::create(&format!(
            "echo {{\"type\":\"result\",\"protocol_version\":4,\"request_id\":\"transcribe-3\",\"operation_id\":\"{operation}\",\"backend\":\"{backend}\",\"text\":\"wrong result\",\"audio_secs\":0.1,\"transcribe_secs\":0.01}}\r\n:hang\r\ngoto hang"
        ));
        let mailbox = WorkerMailbox::start(fixture.start());
        let (fault_tx, fault_rx) = bounded(1);
        mailbox.arm_fault(Arc::new(move || {
            let _ = fault_tx.try_send(());
        }));
        assert!(mailbox.transcribe(&[0; 160], "auto").is_err());
        assert!(mailbox.is_faulted());
        fault_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!mailbox.compatible_with("fixture-model.bin", BackendKind::Cuda, &fixture.script));
    }
}

#[cfg(windows)]
#[test]
fn one_hundred_failed_worker_sessions_are_recoverable() {
    let fixture = WorkerFixture::create("echo not-json\r\n:hang\r\ngoto hang");

    for attempt in 1..=100 {
        let mut session = fixture.start();
        let error = session
            .transcribe(&[0; 160], "auto")
            .expect_err("malformed response must terminate every worker session");

        assert!(
            error.to_string().contains("invalid JSON"),
            "attempt {attempt} returned a different error: {error}"
        );
        assert_session_is_terminated(&mut session);
    }
}

#[cfg(windows)]
#[test]
fn health_ping_uses_the_versioned_request_and_response() {
    let fixture = WorkerFixture::create(
            "echo {\"type\":\"pong\",\"protocol_version\":4,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
        );
    let mut session = fixture.start();

    session.ping().expect("matching pong keeps worker healthy");
    assert!(session.is_alive());
}
