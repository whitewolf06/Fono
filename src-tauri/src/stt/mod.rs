//! Speech-to-Text routing for the embedded Whisper backend and standalone workers.
//!
//! CUDA and Vulkan are compile-time whisper.cpp backends.  Keeping their workers
//! in separate processes makes runtime selection possible without linking two
//! incompatible GPU backends into the Tauri process.

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
use tauri::AppHandle;
#[cfg(not(debug_assertions))]
use tauri::Manager;
use whisper_rs::{SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{AppError, AppResult};
use crate::types::{AccelerationMode, Transcript};

const WORKER_PING_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const WORKER_TRANSCRIBE_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const WORKER_TRANSCRIBE_MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const WORKER_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const WORKER_MAX_STDERR_LINE_BYTES: usize = 4 * 1024;
const WORKER_STDERR_TAIL_LINES: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct WorkerPaths {
    pub cuda: Option<PathBuf>,
    pub vulkan: Option<PathBuf>,
}

impl WorkerPaths {
    pub fn from_resource_dir(resource_dir: &Path) -> Self {
        // Tauri preserves a resource glob's parent directory in bundled apps,
        // while a directly launched development binary may expose the resource
        // directory itself. Support both layouts.
        let nested = resource_dir.join("resources").join("stt-workers");
        let direct = resource_dir.join("stt-workers");
        Self {
            cuda: worker_if_present(nested.join("fono-stt-cuda-worker.exe"))
                .or_else(|| worker_if_present(direct.join("fono-stt-cuda-worker.exe"))),
            vulkan: worker_if_present(nested.join("fono-stt-vulkan-worker.exe"))
                .or_else(|| worker_if_present(direct.join("fono-stt-vulkan-worker.exe"))),
        }
    }

    pub fn capabilities(&self) -> (bool, bool) {
        (
            self.cuda.is_some() || cfg!(feature = "cuda"),
            self.vulkan.is_some() || cfg!(feature = "vulkan"),
        )
    }

    fn candidates(&self, mode: AccelerationMode) -> Vec<EngineCandidate> {
        let embedded_gpu = gpu_backend_compiled();
        match mode {
            AccelerationMode::Auto => {
                let mut candidates = Vec::new();
                if let Some(path) = self.cuda.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Cuda,
                        path: path.clone(),
                    });
                }
                if let Some(path) = self.vulkan.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Vulkan,
                        path: path.clone(),
                    });
                }
                if embedded_gpu {
                    candidates.push(EngineCandidate::Embedded { use_gpu: true });
                }
                candidates.push(EngineCandidate::Embedded { use_gpu: false });
                candidates
            }
            AccelerationMode::Cuda => {
                let mut candidates = Vec::new();
                if let Some(path) = self.cuda.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Cuda,
                        path: path.clone(),
                    });
                }
                if cfg!(feature = "cuda") {
                    candidates.push(EngineCandidate::Embedded { use_gpu: true });
                }
                candidates
            }
            AccelerationMode::Vulkan => {
                let mut candidates = Vec::new();
                if let Some(path) = self.vulkan.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Vulkan,
                        path: path.clone(),
                    });
                }
                if cfg!(feature = "vulkan") {
                    candidates.push(EngineCandidate::Embedded { use_gpu: true });
                }
                candidates
            }
            AccelerationMode::Cpu => vec![EngineCandidate::Embedded { use_gpu: false }],
        }
    }
}

pub fn worker_paths_for_app(app: &AppHandle) -> WorkerPaths {
    #[cfg(debug_assertions)]
    {
        let _ = app;
        WorkerPaths::from_resource_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
    }
    #[cfg(not(debug_assertions))]
    {
        let bundled = app
            .path()
            .resource_dir()
            .map(|path| WorkerPaths::from_resource_dir(&path))
            .unwrap_or_default();
        if bundled.cuda.is_some() || bundled.vulkan.is_some() {
            return bundled;
        }
        // Enables a local `cargo build --release` smoke-test without changing
        // the installed application's lookup path.
        WorkerPaths::from_resource_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
    }
}

fn worker_if_present(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

enum EngineCandidate {
    Embedded { use_gpu: bool },
    Worker { backend: BackendKind, path: PathBuf },
}

enum EngineState {
    Empty,
    Embedded(EmbeddedEngine),
    Worker(Box<WorkerSession>),
}

impl EngineState {
    fn compatible(&mut self, model_path: &str, candidate: &EngineCandidate) -> bool {
        match (self, candidate) {
            (Self::Embedded(current), EngineCandidate::Embedded { use_gpu }) => {
                current.model_path == model_path && current.use_gpu == *use_gpu
            }
            (Self::Worker(current), EngineCandidate::Worker { backend, path }) => {
                current.model_path == model_path
                    && current.backend == *backend
                    && current.path == *path
                    && current.is_alive()
            }
            _ => false,
        }
    }

    fn device(&self) -> String {
        match self {
            Self::Empty => "CPU".into(),
            Self::Embedded(current) => current.device().into(),
            Self::Worker(current) => backend_name(current.backend).into(),
        }
    }
}

struct EmbeddedEngine {
    context: Arc<WhisperContext>,
    model_path: String,
    use_gpu: bool,
}

impl EmbeddedEngine {
    fn load(model_path: &Path, use_gpu: bool) -> AppResult<Self> {
        let model_path_string = model_path.to_string_lossy().to_string();
        let use_gpu = use_gpu && gpu_backend_compiled();
        tracing::info!(
            "loading embedded Whisper model: {} (use_gpu={})",
            model_path.display(),
            use_gpu
        );
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let context = WhisperContext::new_with_params(&model_path_string, params)
            .map_err(|error| AppError::Stt(format!("WhisperContext::new_with_params: {error}")))?;
        Ok(Self {
            context: Arc::new(context),
            model_path: model_path_string,
            use_gpu,
        })
    }

    fn device(&self) -> &'static str {
        if !self.use_gpu {
            return "CPU";
        }
        #[cfg(feature = "cuda")]
        return "CUDA";
        #[cfg(all(feature = "vulkan", not(feature = "cuda")))]
        return "Vulkan";
        #[allow(unreachable_code)]
        "CPU"
    }

    fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        let pcm_f32: Vec<f32> = samples
            .iter()
            .map(|&sample| sample as f32 / i16::MAX as f32)
            .collect();
        let mut params = whisper_rs::FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        match language {
            "" | "auto" => params.set_language(None),
            value => params.set_language(Some(value)),
        }
        let n_threads = num_threads();
        params.set_n_threads(n_threads);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_no_timestamps(true);

        let mut state = self
            .context
            .create_state()
            .map_err(|error| AppError::Stt(format!("create_state: {error}")))?;
        let started = Instant::now();
        state
            .full(params, &pcm_f32)
            .map_err(|error| AppError::Stt(format!("full: {error}")))?;
        let elapsed = started.elapsed().as_secs_f32();
        let audio_secs = samples.len() as f32 / 16_000.0;
        tracing::info!(
            "embedded Whisper finished in {:.2}s (audio={:.2}s, device={})",
            elapsed,
            audio_secs,
            self.device()
        );

        let text = (0..state.full_n_segments())
            .filter_map(|index| state.get_segment(index))
            .filter_map(|segment| segment.to_str_lossy().ok())
            .map(|segment| segment.trim().to_owned())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let detected_language = if language.is_empty() || language == "auto" {
            Some(lang_id_to_str(state.full_lang_id_from_state()))
        } else {
            None
        };
        Ok(Transcript {
            text,
            detected_language,
            transcribe_secs: Some(elapsed),
            audio_secs: Some(audio_secs),
            device: Some(self.device().into()),
        })
    }
}

struct WorkerSession {
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
    fn start(path: PathBuf, backend: BackendKind, model_path: &Path) -> AppResult<Self> {
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
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(
            WORKER_STDERR_TAIL_LINES,
        )));
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

    fn request(&mut self, request: &WorkerRequest) -> AppResult<WorkerResponse> {
        let timeout = worker_request_timeout(request);
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
            Ok(Err(error)) => {
                return Err(self.fail_request(format!("worker stdin: {error}")));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(self.fail_request(format!(
                    "{} worker stdin write timed out after {:.1}s",
                    backend_name(self.backend),
                    timeout.as_secs_f32()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(self.fail_request("worker stdin writer disconnected".into()));
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

    fn transcribe(&mut self, samples: &[i16], language: &str) -> AppResult<Transcript> {
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

fn worker_request_timeout(request: &WorkerRequest) -> Duration {
    match request {
        WorkerRequest::Ping => WORKER_PING_TIMEOUT,
        WorkerRequest::Load { .. } => WORKER_LOAD_TIMEOUT,
        WorkerRequest::Transcribe {
            samples_i16_base64, ..
        } => transcribe_timeout(samples_i16_base64.len()),
    }
}

fn transcribe_timeout(encoded_bytes: usize) -> Duration {
    let approximate_pcm_bytes = (encoded_bytes as u64 / 4).saturating_mul(3);
    let audio_seconds = approximate_pcm_bytes / 2 / 16_000;
    (WORKER_TRANSCRIBE_BASE_TIMEOUT + Duration::from_secs(audio_seconds.saturating_mul(2)))
        .min(WORKER_TRANSCRIBE_MAX_TIMEOUT)
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
            let output = match read_limited_line(&mut reader, WORKER_MAX_RESPONSE_BYTES) {
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
            let line = match read_limited_line(&mut reader, WORKER_MAX_STDERR_LINE_BYTES) {
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
    if tail.len() == WORKER_STDERR_TAIL_LINES {
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

pub struct SttEngine {
    state: Mutex<EngineState>,
}

impl SttEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::Empty),
        }
    }

    pub fn ensure_loaded(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
    ) -> AppResult<()> {
        if !model_path.exists() {
            return Err(AppError::Stt(format!(
                "model file not found: {}",
                model_path.display()
            )));
        }
        let model_path_string = model_path.to_string_lossy().to_string();
        let candidates = worker_paths.candidates(acceleration);
        if candidates.is_empty() {
            return Err(AppError::Stt(format!(
                "{} backend is not packaged in this release",
                acceleration_name(acceleration)
            )));
        }

        let mut state = self.state.lock();
        if candidates
            .iter()
            .any(|candidate| state.compatible(&model_path_string, candidate))
        {
            return Ok(());
        }
        *state = EngineState::Empty;

        let mut errors = Vec::new();
        for candidate in candidates {
            let loaded = match &candidate {
                EngineCandidate::Embedded { use_gpu } => {
                    EmbeddedEngine::load(model_path, *use_gpu).map(EngineState::Embedded)
                }
                EngineCandidate::Worker { backend, path } => {
                    WorkerSession::start(path.clone(), *backend, model_path)
                        .map(Box::new)
                        .map(EngineState::Worker)
                }
            };
            match loaded {
                Ok(loaded) => {
                    tracing::info!("STT backend selected: {}", loaded.device());
                    *state = loaded;
                    return Ok(());
                }
                Err(error) => {
                    errors.push(error.to_string());
                    if acceleration != AccelerationMode::Auto {
                        break;
                    }
                }
            }
        }
        Err(AppError::Stt(format!(
            "could not start {} backend: {}",
            acceleration_name(acceleration),
            errors.join("; ")
        )))
    }

    pub fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        let mut state = self.state.lock();
        let (result, worker_failed) = match &mut *state {
            EngineState::Empty => (Err(AppError::ModelNotLoaded), false),
            EngineState::Embedded(engine) => (engine.transcribe(samples, language), false),
            EngineState::Worker(worker) => {
                let result = worker.transcribe(samples, language);
                let failed = result.is_err();
                (result, failed)
            }
        };
        if worker_failed {
            tracing::warn!("STT worker session failed and will be restarted on the next operation");
            *state = EngineState::Empty;
        }
        result
    }

    pub fn device(&self) -> String {
        self.state.lock().device()
    }

    pub fn is_loaded(&self) -> bool {
        !matches!(&*self.state.lock(), EngineState::Empty)
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn backend_name(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Cuda => "CUDA",
        BackendKind::Vulkan => "Vulkan",
        BackendKind::Cpu => "CPU",
    }
}

fn acceleration_name(acceleration: AccelerationMode) -> &'static str {
    match acceleration {
        AccelerationMode::Auto => "automatic",
        AccelerationMode::Cuda => "CUDA",
        AccelerationMode::Vulkan => "Vulkan",
        AccelerationMode::Cpu => "CPU",
    }
}

fn gpu_backend_compiled() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

fn num_threads() -> std::os::raw::c_int {
    let cpus = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    cpus.clamp(2, 8) as std::os::raw::c_int
}

fn lang_id_to_str(id: i32) -> String {
    match id {
        0 => "en".to_string(),
        1 => "zh".to_string(),
        2 => "de".to_string(),
        3 => "es".to_string(),
        4 => "ru".to_string(),
        5 => "ko".to_string(),
        6 => "fr".to_string(),
        _ => format!("lang#{id}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn auto_prefers_cuda_worker_before_vulkan_worker() {
        let paths = WorkerPaths {
            cuda: Some(PathBuf::from("cuda.exe")),
            vulkan: Some(PathBuf::from("vulkan.exe")),
        };
        let candidates = paths.candidates(AccelerationMode::Auto);
        assert!(matches!(
            candidates.first(),
            Some(EngineCandidate::Worker {
                backend: BackendKind::Cuda,
                ..
            })
        ));
        assert!(matches!(
            candidates.get(1),
            Some(EngineCandidate::Worker {
                backend: BackendKind::Vulkan,
                ..
            })
        ));
    }

    #[test]
    fn explicit_vulkan_never_adds_cpu_fallback() {
        let paths = WorkerPaths::default();
        assert!(paths
            .candidates(AccelerationMode::Vulkan)
            .iter()
            .all(|candidate| {
                !matches!(candidate, EngineCandidate::Embedded { use_gpu: false })
            }));
    }

    #[test]
    fn worker_response_reader_enforces_limit_and_recovers_at_next_line() {
        let mut input = Cursor::new(b"response-too-long\nok\n");

        let error = read_limited_line(&mut input, 8).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(read_limited_line(&mut input, 8).unwrap(), Some("ok".into()));
        assert_eq!(read_limited_line(&mut input, 8).unwrap(), None);
    }

    #[test]
    fn worker_transcription_timeout_is_bounded() {
        assert_eq!(transcribe_timeout(0), WORKER_TRANSCRIBE_BASE_TIMEOUT);
        assert_eq!(
            transcribe_timeout(usize::MAX),
            WORKER_TRANSCRIBE_MAX_TIMEOUT
        );
    }
}
