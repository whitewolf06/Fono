//! Supervised protocol client for standalone STT workers.
//!
//! A worker owns its child process and all three stdio threads. Any failed
//! request tears down that complete session, so the router can safely create a
//! fresh one for the following operation.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
use crate::types::{ModelId, SttModelSelection, Transcript};
use fono_core::OperationCancellation;

use super::recognizer::{
    RecognitionOptions, Recognizer, RecognizerCapabilities, RecognizerLoad, WindowOptions,
};

#[path = "worker_window.rs"]
mod window;

const PING_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSCRIBE_BASE_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSCRIBE_MAX_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAILBOX_HEALTH_TIMEOUT: Duration = Duration::from_millis(100);
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);
const FORCE_EXIT_TIMEOUT: Duration = Duration::from_millis(500);
const THREAD_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
const MAILBOX_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
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
            | WorkerRequest::CancelRequest { .. }
            | WorkerRequest::Shutdown { .. } => self.ping,
            WorkerRequest::Load { .. } => self.load,
            WorkerRequest::Transcribe {
                samples_i16_base64, ..
            }
            | WorkerRequest::TranscribeWindow {
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
    child: Arc<WorkerChild>,
    cleanup: Arc<WorkerCleanupSupervisor>,
    transport_stopping: Arc<AtomicBool>,
    cleanup_ticket: Option<Arc<AtomicBool>>,
    stdin_tx: Option<Sender<WorkerWrite>>,
    stdin_thread: Option<JoinHandle<()>>,
    stdout_rx: Receiver<WorkerOutput>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_thread: Option<JoinHandle<()>>,
    next_request_id: u64,
    timeouts: WorkerTimeouts,
    supports_decoding_options: bool,
    // A test observer distinguishes complete transport writes from requests
    // cancelled before transport. This counter is absent from production.
    #[cfg(test)]
    written_frames: Arc<AtomicUsize>,
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

/// Serializes a standalone workers mutable protocol session on one owner
/// thread. Callers never lock the session while inference is in progress.
pub(super) struct WorkerMailbox {
    backend: BackendKind,
    path: PathBuf,
    model_path: String,
    model: ModelId,
    supports_decoding_options: bool,
    command_tx: Sender<WorkerCommand>,
    owner_thread: Mutex<Option<JoinHandle<()>>>,
    busy: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    health_pending: AtomicBool,
    closing: AtomicBool,
    child: Arc<WorkerChild>,
    cleanup: Arc<WorkerCleanupSupervisor>,
    transport_stopping: Arc<AtomicBool>,
    fault: Arc<super::recovery::FaultSignal>,
}

enum WorkerCommand {
    Window {
        samples: Vec<i16>,
        language: String,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: Vec<String>,
        context: Option<String>,
        operation_id: u64,
        cancellation: OperationCancellation,
        audio_start_sample: u64,
        response_tx: Sender<AppResult<fono_stt_protocol::WindowTranscript>>,
        activity: crate::application::updates::activity::ActivityLease<'static>,
    },
    Transcribe {
        samples: Vec<i16>,
        language: String,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: Vec<String>,
        cancellation: OperationCancellation,
        response_tx: Sender<AppResult<Transcript>>,
        activity: crate::application::updates::activity::ActivityLease<'static>,
    },
    Ping {
        response_tx: Sender<AppResult<()>>,
        activity: crate::application::updates::activity::ActivityLease<'static>,
    },
    Shutdown {
        response_tx: Sender<WorkerCleanupStatus>,
    },
}

pub(super) enum MailboxHealth {
    Busy,
    Ready,
    Failed(AppError),
}

include!("worker_session.rs");
include!("worker_request.rs");
include!("worker_mailbox.rs");
include!("worker_transport.rs");
include!("worker_cleanup.rs");
#[cfg(all(test, windows))]
#[path = "lifecycle_soak.rs"]
mod lifecycle_soak;
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
