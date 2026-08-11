//! Supervised protocol client for standalone STT workers.
//!
//! A worker owns its child process and all three stdio threads. Any failed
//! request tears down that complete session, so the router can safely create a
//! fresh one for the following operation.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine;
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, SendTimeoutError, Sender};
use fono_stt_protocol::{
    BackendKind, RequestMeta, WorkerRequest, WorkerResponse, MAX_REQUEST_FRAME_BYTES,
    MAX_RESPONSE_FRAME_BYTES, PROTOCOL_VERSION,
};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::operation::OperationCancellation;
use crate::types::Transcript;

const PING_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSCRIBE_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSCRIBE_MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAILBOX_HEALTH_TIMEOUT: Duration = Duration::from_millis(100);
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
            WorkerRequest::Hello { .. }
            | WorkerRequest::Ping { .. }
            | WorkerRequest::Shutdown { .. } => self.ping,
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

/// Serializes a standalone worker's mutable protocol session on one owner
/// thread. Callers never lock the session while inference is in progress.
pub(super) struct WorkerMailbox {
    backend: BackendKind,
    path: PathBuf,
    model_path: String,
    command_tx: Sender<WorkerCommand>,
    owner_thread: Mutex<Option<JoinHandle<()>>>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    health_pending: AtomicBool,
}

enum WorkerCommand {
    Transcribe {
        samples: Vec<i16>,
        language: String,
        cancellation: OperationCancellation,
        response_tx: Sender<AppResult<Transcript>>,
    },
    Ping {
        response_tx: Sender<AppResult<()>>,
    },
    Shutdown,
}

pub(super) enum MailboxHealth {
    Busy,
    Ready,
    Failed(AppError),
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
        let hello_meta = worker.next_meta("hello", None);
        match worker.request(&WorkerRequest::Hello { meta: hello_meta })? {
            WorkerResponse::Ready {
                backend: actual,
                capabilities,
                ..
            } if actual == backend
                && capabilities.protocol_version == PROTOCOL_VERSION
                && capabilities.maximum_request_bytes >= MAX_REQUEST_FRAME_BYTES
                && capabilities.maximum_response_bytes >= MAX_RESPONSE_FRAME_BYTES => {}
            other => {
                return Err(AppError::Stt(format!(
                    "{} worker returned an unexpected handshake: {other:?}",
                    backend_name(backend)
                )))
            }
        }
        let load_meta = worker.next_meta("load", None);
        match worker.request(&WorkerRequest::Load {
            meta: load_meta,
            model_path: worker.model_path.clone(),
        })? {
            WorkerResponse::ModelLoaded {
                backend: actual, ..
            } if actual == backend => {}
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

    pub(super) fn device(&self) -> &'static str {
        backend_name(self.backend)
    }

    pub(super) fn ping(&mut self) -> AppResult<()> {
        let meta = self.next_meta("ping", None);
        match self.request(&WorkerRequest::Ping { meta: meta.clone() })? {
            WorkerResponse::Pong {
                backend,
                request_id,
                ..
            } if backend == self.backend && request_id == meta.request_id => Ok(()),
            other => Err(AppError::Stt(format!(
                "{} worker returned an unexpected health response: {other:?}",
                backend_name(self.backend)
            ))),
        }
    }

    #[cfg(test)]
    pub(super) fn transcribe(&mut self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, None)
    }

    pub(super) fn transcribe_cancellable(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_with_cancellation(samples, language, Some(cancellation))
    }

    fn transcribe_with_cancellation(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<Transcript> {
        let meta = self.next_meta("transcribe", None);
        let request_id = meta.request_id.clone();
        let operation_id = format!("dictation-{request_id}");
        let response = self.request_with_cancellation(
            &WorkerRequest::Transcribe {
                meta: RequestMeta {
                    operation_id: Some(operation_id.clone()),
                    ..meta
                },
                model_path: self.model_path.clone(),
                language: language.into(),
                samples_i16_base64: encode_samples_i16_base64(samples),
            },
            cancellation,
        )?;
        match response {
            WorkerResponse::Result {
                request_id: actual_request_id,
                operation_id: actual_operation_id,
                text,
                audio_secs,
                transcribe_secs,
                backend,
                ..
            } if backend == self.backend
                && actual_request_id == request_id
                && actual_operation_id == operation_id =>
            {
                Ok(Transcript {
                    text,
                    detected_language: None,
                    transcribe_secs: Some(transcribe_secs),
                    audio_secs: Some(audio_secs),
                    device: Some(backend_name(backend).into()),
                })
            }
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
        self.request_with_cancellation(request, None)
    }

    fn request_with_cancellation(
        &mut self,
        request: &WorkerRequest,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<WorkerResponse> {
        let timeout = self.timeouts.for_request(request);
        let deadline = Instant::now() + timeout;
        if cancellation.is_some_and(OperationCancellation::is_cancelled) {
            return Err(self.cancel_request());
        }
        let line = serde_json::to_string(request)
            .map_err(|error| AppError::Stt(format!("worker request serialization: {error}")))?;
        if line.len() > MAX_REQUEST_FRAME_BYTES {
            return Err(self.fail_request(format!(
                "worker request exceeds {MAX_REQUEST_FRAME_BYTES} bytes"
            )));
        }
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

        let response = loop {
            if cancellation.is_some_and(OperationCancellation::is_cancelled) {
                return Err(self.cancel_request());
            }
            let remaining = remaining_until(deadline);
            if remaining.is_zero() {
                return Err(self.fail_request(format!(
                    "{} worker request timed out after {:.1}s",
                    backend_name(self.backend),
                    timeout.as_secs_f32()
                )));
            }
            match self
                .stdout_rx
                .recv_timeout(remaining.min(CANCELLATION_POLL_INTERVAL))
            {
                Ok(WorkerOutput::Line(response)) => break response,
                Ok(WorkerOutput::Eof) => {
                    return Err(
                        self.fail_request("worker exited before returning a response".into())
                    )
                }
                Ok(WorkerOutput::Error(error)) => {
                    return Err(self.fail_request(format!("worker stdout: {error}")))
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.fail_request("worker stdout reader disconnected".into()))
                }
            }
        };

        let response: WorkerResponse = serde_json::from_str(response.trim())
            .map_err(|error| self.fail_request(format!("worker returned invalid JSON: {error}")))?;
        if response.protocol_version() != PROTOCOL_VERSION {
            return Err(self.fail_request(format!(
                "worker returned protocol version {}; expected {PROTOCOL_VERSION}",
                response.protocol_version()
            )));
        }
        if response.request_id() != request.meta().request_id {
            return Err(self.fail_request(format!(
                "worker returned request_id {}; expected {}",
                response.request_id(),
                request.meta().request_id
            )));
        }
        Ok(response)
    }

    fn next_meta(&mut self, kind: &str, operation_id: Option<String>) -> RequestMeta {
        self.next_request_id = self.next_request_id.wrapping_add(1);
        RequestMeta::new(format!("{kind}-{}", self.next_request_id), operation_id)
    }

    #[cfg(test)]
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

    fn cancel_request(&mut self) -> AppError {
        let error = self.fail_request("worker request cancelled".into());
        AppError::Cancelled(error.to_string())
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

    fn request_graceful_shutdown(&mut self) {
        if self.stdin_tx.is_none() || !matches!(self.child.try_wait(), Ok(None)) {
            return;
        }
        let request = WorkerRequest::Shutdown {
            meta: self.next_meta("shutdown", None),
        };
        let Ok(line) = serde_json::to_string(&request) else {
            return;
        };
        let (result_tx, result_rx) = bounded(1);
        let Some(stdin_tx) = self.stdin_tx.as_ref() else {
            return;
        };
        if stdin_tx
            .send_timeout(WorkerWrite { line, result_tx }, Duration::from_millis(500))
            .is_err()
            || !matches!(
                result_rx.recv_timeout(Duration::from_millis(500)),
                Ok(Ok(()))
            )
        {
            return;
        }
        if let Ok(WorkerOutput::Line(line)) =
            self.stdout_rx.recv_timeout(Duration::from_millis(500))
        {
            let acknowledged =
                serde_json::from_str::<WorkerResponse>(&line).is_ok_and(|response| {
                    response.protocol_version() == PROTOCOL_VERSION
                        && response.request_id() == request.meta().request_id
                        && matches!(response, WorkerResponse::ShuttingDown { .. })
                });
            if acknowledged {
                let _ = self.child.wait();
            }
        }
    }
}

fn encode_samples_i16_base64(samples: &[i16]) -> String {
    let mut pcm = Vec::with_capacity(std::mem::size_of_val(samples));
    for sample in samples {
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(pcm)
}

impl WorkerMailbox {
    pub(super) fn start(session: WorkerSession) -> Self {
        let backend = session.backend;
        let path = session.path.clone();
        let model_path = session.model_path.clone();
        let (command_tx, command_rx) = crossbeam_channel::unbounded();
        let busy = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let thread_busy = Arc::clone(&busy);
        let thread_alive = Arc::clone(&alive);
        let owner_thread = thread::spawn(move || {
            let mut session = session;
            while let Ok(command) = command_rx.recv() {
                let keep_running = match command {
                    WorkerCommand::Transcribe {
                        samples,
                        language,
                        cancellation,
                        response_tx,
                    } => {
                        thread_busy.store(true, Ordering::Release);
                        let result =
                            session.transcribe_cancellable(&samples, &language, &cancellation);
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = result.is_ok();
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Ping { response_tx } => {
                        thread_busy.store(true, Ordering::Release);
                        let result = session.ping();
                        thread_busy.store(false, Ordering::Release);
                        let keep_running = result.is_ok();
                        let _ = response_tx.send(result);
                        keep_running
                    }
                    WorkerCommand::Shutdown => false,
                };
                if !keep_running {
                    break;
                }
            }
            thread_busy.store(false, Ordering::Release);
            thread_alive.store(false, Ordering::Release);
        });
        Self {
            backend,
            path,
            model_path,
            command_tx,
            owner_thread: Mutex::new(Some(owner_thread)),
            busy,
            alive,
            health_pending: AtomicBool::new(false),
        }
    }

    pub(super) fn compatible_with(
        &self,
        model_path: &str,
        backend: BackendKind,
        path: &Path,
    ) -> bool {
        self.model_path == model_path
            && self.backend == backend
            && self.path == path
            && self.alive.load(Ordering::Acquire)
    }

    pub(super) fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_cancellable(samples, language, &OperationCancellation::default())
    }

    pub(super) fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "worker transcription cancelled before start".into(),
            ));
        }
        let (response_tx, response_rx) = bounded(1);
        self.command_tx
            .send(WorkerCommand::Transcribe {
                samples: samples.to_vec(),
                language: language.into(),
                cancellation: cancellation.clone(),
                response_tx,
            })
            .map_err(|_| AppError::Stt("worker mailbox is unavailable".into()))?;
        loop {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("worker transcription cancelled".into()));
            }
            match response_rx.recv_timeout(CANCELLATION_POLL_INTERVAL) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(AppError::Stt(
                        "worker mailbox stopped before responding".into(),
                    ))
                }
            }
        }
    }

    pub(super) fn health(&self) -> MailboxHealth {
        if self.busy.load(Ordering::Acquire)
            || self
                .health_pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return MailboxHealth::Busy;
        }
        let (response_tx, response_rx) = bounded(1);
        let send = self.command_tx.send(WorkerCommand::Ping { response_tx });
        if send.is_err() {
            self.health_pending.store(false, Ordering::Release);
            return MailboxHealth::Failed(AppError::Stt("worker mailbox is unavailable".into()));
        }
        let health = match response_rx.recv_timeout(MAILBOX_HEALTH_TIMEOUT) {
            Ok(Ok(())) => MailboxHealth::Ready,
            Ok(Err(error)) => MailboxHealth::Failed(error),
            Err(RecvTimeoutError::Timeout) => MailboxHealth::Busy,
            Err(RecvTimeoutError::Disconnected) => MailboxHealth::Failed(AppError::Stt(
                "worker mailbox stopped during health probe".into(),
            )),
        };
        self.health_pending.store(false, Ordering::Release);
        health
    }
}

impl Drop for WorkerMailbox {
    fn drop(&mut self) {
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(thread) = self.owner_thread.lock().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        self.request_graceful_shutdown();
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
            let output = match read_limited_line(&mut reader, MAX_RESPONSE_FRAME_BYTES) {
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
                    "@echo off\r\nset /p request=\r\necho {{\"type\":\"ready\",\"protocol_version\":2,\"request_id\":\"hello-1\",\"backend\":\"cuda\",\"capabilities\":{{\"protocol_version\":2,\"supports_health\":true,\"supports_shutdown\":true,\"maximum_request_bytes\":16777216,\"maximum_response_bytes\":1048576}}}}\r\nset /p request=\r\necho {{\"type\":\"model_loaded\",\"protocol_version\":2,\"request_id\":\"load-2\",\"backend\":\"cuda\"}}\r\nset /p request=\r\n{after_load}\r\n"
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
            "echo {\"type\":\"result\",\"protocol_version\":2,\"request_id\":\"foreign\",\"operation_id\":\"dictation-transcribe-3\",\"text\":\"x\",\"audio_secs\":0.01,\"transcribe_secs\":0.01,\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
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
            "echo {\"type\":\"pong\",\"protocol_version\":2,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
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
            "echo {\"type\":\"pong\",\"protocol_version\":2,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\n:hang\r\ngoto hang",
        );
        let mut session = fixture.start();

        session.ping().expect("matching pong keeps worker healthy");
        assert!(session.is_alive());
    }
}
