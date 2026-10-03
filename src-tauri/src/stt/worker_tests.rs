use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::operation::{OperationCoordinator, OperationSource};

// Parallel workspace tests can briefly delay Windows cmd fixture startup;
// keep this distinct from the production request deadlines.
const TEST_TIMEOUT: Duration = Duration::from_secs(1);
const TEST_TIMEOUTS: WorkerTimeouts = WorkerTimeouts {
    ping: TEST_TIMEOUT,
    load: TEST_TIMEOUT,
    transcribe_base: TEST_TIMEOUT,
    transcribe_max: TEST_TIMEOUT,
};

struct WorkerFixture {
    directory: PathBuf,
    script: PathBuf,
}

impl WorkerFixture {
    fn create(after_load: &str) -> Self {
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
                    "@echo off\r\nset /p request=\r\necho {{\"type\":\"ready\",\"protocol_version\":3,\"request_id\":\"hello-1\",\"backend\":\"cuda\",\"capabilities\":{{\"protocol_version\":3,\"supports_health\":true,\"supports_shutdown\":true,\"supports_window\":true,\"supports_cancel\":true,\"supports_token_timestamps\":true,\"maximum_request_bytes\":16777216,\"maximum_response_bytes\":1048576}}}}\r\nset /p request=\r\necho {{\"type\":\"model_loaded\",\"protocol_version\":3,\"request_id\":\"load-2\",\"backend\":\"cuda\"}}\r\nset /p request=\r\n{after_load}\r\n"
                ),
            )
            .expect("write worker fixture");
        Self { directory, script }
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
    assert!(matches!(session.child.try_wait(), Ok(Some(_))));
    assert!(session.stdin_tx.is_none());
    assert!(session.stdin_thread.is_none());
    assert!(session.stdout_thread.is_none());
    assert!(session.stderr_thread.is_none());
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
            samples_i16_base64: encoded,
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
        samples_i16_base64: encoded,
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
    let fixture = WorkerFixture::create("exit /b 17");
    let mut session = fixture.start();

    let error = session.transcribe(&[0; 160], "auto").unwrap_err();

    assert!(error
        .to_string()
        .contains("exited before returning a response"));
    assert_session_is_terminated(&mut session);
}

#[cfg(windows)]
#[test]
fn foreign_request_id_terminates_the_session() {
    let fixture = WorkerFixture::create(
            "echo {\"type\":\"result\",\"protocol_version\":3,\"request_id\":\"foreign\",\"operation_id\":\"dictation-transcribe-3\",\"text\":\"x\",\"audio_secs\":0.01,\"transcribe_secs\":0.01,\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
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
            "echo {\"type\":\"pong\",\"protocol_version\":3,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
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
            "echo {\"type\":\"pong\",\"protocol_version\":3,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
        );
    let mut session = fixture.start();

    session.ping().expect("matching pong keeps worker healthy");
    assert!(session.is_alive());
}
