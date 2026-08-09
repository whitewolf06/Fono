//! Supervised protocol client for standalone STT workers.
//!
//! A worker owns its child process and all three stdio threads. Any failed
//! request tears down that complete session, so the router can safely create a
//! fresh one for the following operation.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine;
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, SendTimeoutError, Sender};
use fono_stt_protocol::{BackendKind, WorkerRequest, WorkerResponse};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::Transcript;

const PING_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSCRIBE_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSCRIBE_MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_STDERR_LINE_BYTES: usize = 4 * 1024;
const STDERR_TAIL_LINES: usize = 32;

#[derive(Clone, Copy)]
struct WorkerTimeouts {
    ping: Duration,
    load: Duration,
    transcribe_base: Duration,
    transcribe_max: Duration,
}

const PRODUCTION_TIMEOUTS: WorkerTimeouts = WorkerTimeouts {
    ping: PING_TIMEOUT,
    load: LOAD_TIMEOUT,
    transcribe_base: TRANSCRIBE_BASE_TIMEOUT,
    transcribe_max: TRANSCRIBE_MAX_TIMEOUT,
};

impl WorkerTimeouts {
    fn for_request(self, request: &WorkerRequest) -> Duration {
        match request {
            WorkerRequest::Ping => self.ping,
            WorkerRequest::Load { .. } => self.load,
            WorkerRequest::Transcribe {
                samples_i16_base64, ..
            } => self.transcribe_timeout(samples_i16_base64.len()),
        }
    }

    fn transcribe_timeout(self, encoded_bytes: usize) -> Duration {
        let approximate_pcm_bytes = (encoded_bytes as u64 / 4).saturating_mul(3);
        let audio_seconds = approximate_pcm_bytes / 2 / 16_000;
        (self.transcribe_base + Duration::from_secs(audio_seconds.saturating_mul(2)))
            .min(self.transcribe_max)
    }
}

pub(super) struct WorkerSession {
    backend: BackendKind,
    path: PathBuf,
    model_path: String,
    child: Child,
    stdin_tx: Option<Sender<WorkerWrite>>,
    stdin_thread: Option<JoinHandle<()>>,
    stdout_rx: Receiver<WorkerOutput>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_thread: Option<JoinHandle<()>>,
    next_request_id: u64,
    timeouts: WorkerTimeouts,
}

enum WorkerOutput {
    Line(String),
    Eof,
    Error(String),
}

struct WorkerWrite {
    line: String,
    result_tx: Sender<Result<(), String>>,
}

impl WorkerSession {
    pub(super) fn start(path: PathBuf, backend: BackendKind, model_path: &Path) -> AppResult<Self> {
        Self::start_with_timeouts(path, backend, model_path, PRODUCTION_TIMEOUTS)
    }

    fn start_with_timeouts(
        path: PathBuf,
        backend: BackendKind,
        model_path: &Path,
        timeouts: WorkerTimeouts,
    ) -> AppResult<Self> {
        let mut command = Command::new(&path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // whisper.cpp emits progress and backend diagnostics to stderr.
            // Drain it on a dedicated thread and retain only a bounded tail.
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // GPU workers are helper processes, not interactive terminals.
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|error| {
            AppError::Stt(format!(
                "cannot start {} worker: {error}",
                backend_name(backend)
            ))
        })?;
        let Some(stdin) = child.stdin.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stdin is unavailable".into()));
        };
        let Some(stdout) = child.stdout.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stdout is unavailable".into()));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_child(&mut child);
            return Err(AppError::Stt("worker stderr is unavailable".into()));
        };
        let (stdout_rx, stdout_thread) = spawn_stdout_reader(stdout);
        let (stdin_tx, stdin_thread) = spawn_stdin_writer(stdin);
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_thread = spawn_stderr_reader(stderr, stderr_tail.clone());
        let mut worker = Self {
            backend,
            path,
            model_path: model_path.to_string_lossy().to_string(),
            child,
            stdin_tx: Some(stdin_tx),
            stdin_thread: Some(stdin_thread),
            stdout_rx,
            stdout_thread: Some(stdout_thread),
            stderr_tail,
            stderr_thread: Some(stderr_thread),
            next_request_id: 0,
            timeouts,
        };
        match worker.request(&WorkerRequest::Ping)? {
            WorkerResponse::Ready { backend: actual } if actual == backend => {}
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected handshake: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        match worker.request(&WorkerRequest::Load {
            model_path: worker.model_path.clone(),
        })? {
            WorkerResponse::ModelLoaded { backend: actual } if actual == backend => {}
            WorkerResponse::Error { message, .. } => {
                return Err(AppError::Stt(format!(
                    "{} worker could not load model: {message}",
                    backend_name(backend)
                )))
            }
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected load result: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        tracing::info!("{} worker loaded the Whisper model", backend_name(backend));
        Ok(worker)
    }

    pub(super) fn compatible_with(
        &mut self,
        model_path: &str,
        backend: BackendKind,
        path: &Path,
    ) -> bool {
        self.model_path == model_path
            && self.backend == backend
            && self.path == path
            && self.is_alive()
    }

    pub(super) fn device(&self) -> &'static str {
        backend_name(self.backend)
    }

    pub(super) fn transcribe(&mut self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        let mut pcm = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = format!("dictation-{}", self.next_request_id);
        let response = self.request(&WorkerRequest::Transcribe {
            id: request_id.clone(),
            model_path: self.model_path.clone(),
            language: language.into(),
            samples_i16_base64: base64::engine::general_purpose::STANDARD.encode(pcm),
        })?;
        match response {
            WorkerResponse::Result {
                id,
                text,
                audio_secs,
                transcribe_secs,
                backend,
                ..
            } if backend == self.backend && id == request_id => Ok(Transcript {
                text,
                detected_language: None,
                transcribe_secs: Some(transcribe_secs),
                audio_secs: Some(audio_secs),
                device: Some(backend_name(backend).into()),
            }),
            WorkerResponse::Error { message, .. } => Err(AppError::Stt(format!(
                "{} worker transcription failed: {message}",
                backend_name(self.backend)
            ))),
            other => Err(AppError::Stt(format!(
                "{} worker returned an unexpected transcription result: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }

    fn request(&mut self, request: &WorkerRequest) -> AppResult<WorkerResponse> {
        let timeout = self.timeouts.for_request(request);
        let deadline = Instant::now() + timeout;
        let line = serde_json::to_string(request)
            .map_err(|error| AppError::Stt(format!("worker request serialization: {error}")))?;
        let (result_tx, result_rx) = bounded(1);
        let write = WorkerWrite { line, result_tx };
        let Some(stdin_tx) = self.stdin_tx.as_ref() else {
            return Err(self.fail_request("worker stdin is closed".into()));
        };
        if let Err(error) = stdin_tx.send_timeout(write, remaining_until(deadline)) {
            let message = match error {
                SendTimeoutError::Timeout(_) => "worker stdin queue timed out",
                SendTimeoutError::Disconnected(_) => "worker stdin writer disconnected",
            };
            return Err(self.fail_request(message.into()));
        }
        match result_rx.recv_timeout(remaining_until(deadline)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(self.fail_request(format!("worker stdin: {error}"))),
            Err(RecvTimeoutError::Timeout) => {
                return Err(self.fail_request(format!(
                    "{} worker stdin write timed out after {:.1}s",
                    backend_name(self.backend),
                    timeout.as_secs_f32()
                )))
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(self.fail_request("worker stdin writer disconnected".into()))
            }
        }

        let response = match self.stdout_rx.recv_timeout(remaining_until(deadline)) {
            Ok(WorkerOutput::Line(response)) => response,
            Ok(WorkerOutput::Eof) => {
                return Err(self.fail_request("worker exited before returning a response".into()))
            }
            Ok(WorkerOutput::Error(error)) => {
                return Err(self.fail_request(format!("worker stdout: {error}")))
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(self.fail_request(format!(
                    "{} worker request timed out after {:.1}s",
                    backend_name(self.backend),
                    timeout.as_secs_f32()
                )))
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(self.fail_request("worker stdout reader disconnected".into()))
            }
        };

        serde_json::from_str(response.trim())
            .map_err(|error| self.fail_request(format!("worker returned invalid JSON: {error}")))
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
            && self
                .stdin_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            && self
                .stdout_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
            && self
                .stderr_thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
    }

    fn fail_request(&mut self, message: String) -> AppError {
        self.terminate();
        let diagnostics = self.stderr_diagnostics();
        if diagnostics.is_empty() {
            AppError::Stt(message)
        } else {
            AppError::Stt(format!("{message}; worker stderr: {diagnostics}"))
        }
    }

    fn stderr_diagnostics(&self) -> String {
        self.stderr_tail
            .lock()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn terminate(&mut self) {
        self.stdin_tx.take();
        terminate_child(&mut self.child);
        if let Some(thread) = self.stdin_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
fn transcribe_timeout(encoded_bytes: usize) -> Duration {
    PRODUCTION_TIMEOUTS.transcribe_timeout(encoded_bytes)
}

fn terminate_child(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn spawn_stdin_writer(stdin: ChildStdin) -> (Sender<WorkerWrite>, JoinHandle<()>) {
    let (sender, receiver) = bounded::<WorkerWrite>(1);
    let thread = thread::spawn(move || {
        let mut stdin = stdin;
        while let Ok(write) = receiver.recv() {
            let result = writeln!(stdin, "{}", write.line)
                .and_then(|_| stdin.flush())
                .map_err(|error| error.to_string());
            let failed = result.is_err();
            let _ = write.result_tx.try_send(result);
            if failed {
                break;
            }
        }
    });
    (sender, thread)
}

fn spawn_stdout_reader(stdout: ChildStdout) -> (Receiver<WorkerOutput>, JoinHandle<()>) {
    let (sender, receiver) = bounded(8);
    let thread = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let output = match read_limited_line(&mut reader, MAX_RESPONSE_BYTES) {
                Ok(Some(line)) => WorkerOutput::Line(line),
                Ok(None) => WorkerOutput::Eof,
                Err(error) => WorkerOutput::Error(error.to_string()),
            };
            let terminal = !matches!(output, WorkerOutput::Line(_));
            if sender.try_send(output).is_err() || terminal {
                break;
            }
        }
    });
    (receiver, thread)
}

fn spawn_stderr_reader(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        loop {
            let line = match read_limited_line(&mut reader, MAX_STDERR_LINE_BYTES) {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    let mut tail = tail.lock();
                    push_stderr_line(&mut tail, format!("<stderr reader error: {error}>"));
                    continue;
                }
            };
            push_stderr_line(&mut tail.lock(), line);
        }
    })
}

fn push_stderr_line(tail: &mut VecDeque<String>, line: String) {
    if tail.len() == STDERR_TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn read_limited_line<R: BufRead>(
    reader: &mut R,
    maximum_bytes: usize,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut exceeded_limit = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                if exceeded_limit {
                    return Err(std::io::Error::new(
                        ErrorKind::InvalidData,
                        format!("worker line exceeds {maximum_bytes} bytes"),
                    ));
                }
                return Ok(None);
            }
            break;
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !exceeded_limit && bytes.len().saturating_add(consumed) > maximum_bytes {
            exceeded_limit = true;
            bytes.clear();
        }
        if !exceeded_limit {
            bytes.extend_from_slice(&available[..consumed]);
        }
        reader.consume(consumed);
        if newline.is_some() {
            if exceeded_limit {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("worker line exceeds {maximum_bytes} bytes"),
                ));
            }
            break;
        }
    }

    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))
}

fn backend_name(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Cuda => "CUDA",
        BackendKind::Vulkan => "Vulkan",
        BackendKind::Cpu => "CPU",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_millis(250);
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
                    "@echo off\r\nset /p request=\r\necho {{\"type\":\"ready\",\"backend\":\"cuda\"}}\r\nset /p request=\r\necho {{\"type\":\"model_loaded\",\"backend\":\"cuda\"}}\r\nset /p request=\r\n{after_load}\r\n"
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
    fn hung_worker_response_respects_deadline_and_terminates_the_session() {
        let fixture = WorkerFixture::create(":hang\r\ngoto hang");
        let mut session = fixture.start();
        let started = Instant::now();

        let error = session.transcribe(&[0; 160], "auto").unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.to_string().contains("timed out"));
        assert_session_is_terminated(&mut session);
    }
}
