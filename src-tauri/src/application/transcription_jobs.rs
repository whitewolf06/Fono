//! Bounded in-memory job queue for non-interactive transcription.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fono_core::OperationCancellation;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::application::transcription_contract::{
    TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
};
use crate::application::transcription_service::{TranscriptionRuntime, TranscriptionService};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Preparing,
    Transcribing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionJob {
    pub id: String,
    #[serde(default = "default_requested_language")]
    pub requested_language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    pub state: JobState,
    pub created_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub result: Option<TranscriptionResult>,
    pub error: Option<TranscriptionServiceError>,
}

impl TranscriptionJob {
    fn queued(id: String, requested_language: String) -> Self {
        Self {
            id,
            requested_language,
            file_name: None,
            progress: None,
            state: JobState::Queued,
            created_at_ms: now_epoch_ms(),
            started_at_ms: None,
            finished_at_ms: None,
            result: None,
            error: None,
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        )
    }
}

fn default_requested_language() -> String {
    "auto".into()
}

type JobObserver = Arc<dyn Fn(TranscriptionJob) + Send + Sync>;

/// A non-persistent view of the in-memory job queue for the desktop UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TranscriptionQueueSnapshot {
    pub capacity: usize,
    pub queued: usize,
    pub preparing: usize,
    pub transcribing: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub jobs: Vec<TranscriptionJob>,
}

pub trait InteractiveActivity: Send + Sync {
    fn is_active(&self) -> bool;
}

/// Transport-neutral port implemented by the queue and consumed by HTTP.
pub trait TranscriptionJobs: Send + Sync {
    fn submit_job(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError>;
    fn submit_file(
        &self,
        _path: PathBuf,
        _language: String,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        Err(TranscriptionServiceError::InvalidRequest(
            "file input is unavailable".into(),
        ))
    }
    fn get_job(&self, id: &str) -> Option<TranscriptionJob>;
    fn cancel_job(&self, id: &str) -> Option<TranscriptionJob>;
    fn run_next_job(&self) -> Option<TranscriptionJob>;
    fn cancel_all_jobs(&self);
    fn snapshot(&self) -> TranscriptionQueueSnapshot {
        TranscriptionQueueSnapshot::default()
    }
}

/// Background runner for one bounded queue. It owns no STT runtime: every
/// execution still goes through the queue's shared-operation adapter.
pub struct TranscriptionJobWorker {
    stopped: Arc<std::sync::atomic::AtomicBool>,
    jobs: Arc<dyn TranscriptionJobs>,
}

impl TranscriptionJobWorker {
    pub fn start(jobs: Arc<dyn TranscriptionJobs>) -> std::io::Result<Self> {
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped_for_thread = Arc::clone(&stopped);
        let jobs_for_thread = Arc::clone(&jobs);
        std::thread::Builder::new()
            .name("fono-transcription-jobs".into())
            .spawn(move || {
                while !stopped_for_thread.load(Ordering::Acquire) {
                    if jobs_for_thread.run_next_job().is_none() {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            })?;
        Ok(Self { stopped, jobs })
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.jobs.cancel_all_jobs();
    }
}

impl Drop for TranscriptionJobWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Default)]
pub struct NoInteractiveActivity;

impl InteractiveActivity for NoInteractiveActivity {
    fn is_active(&self) -> bool {
        false
    }
}

enum JobInput {
    Pcm(TranscriptionRequest),
    File { path: PathBuf, language: String },
}
struct QueuedJob {
    input: Option<JobInput>,
    _activity: crate::application::updates::activity::ActivityLease<'static>,
    cancellation: OperationCancellation,
}

#[derive(Default)]
struct QueueState {
    jobs: BTreeMap<String, TranscriptionJob>,
    pending: VecDeque<String>,
    work: BTreeMap<String, QueuedJob>,
}

pub struct TranscriptionJobQueue<R, G> {
    service: TranscriptionService<R>,
    interactive: G,
    capacity: usize,
    // Only owned data/atomic mutations happen under this nonpoisoning lock.
    // Runtime and observer callbacks always execute after dropping the guard.
    state: Mutex<QueueState>,
    observer: Option<JobObserver>,
}

const MAX_RETAINED_TERMINAL_JOBS: usize = 50;

impl<R, G> TranscriptionJobQueue<R, G>
where
    R: TranscriptionRuntime,
    G: InteractiveActivity,
{
    pub fn new(service: TranscriptionService<R>, interactive: G, capacity: usize) -> Self {
        Self {
            service,
            interactive,
            capacity,
            state: Mutex::new(QueueState::default()),
            observer: None,
        }
    }

    pub fn with_observer(mut self, observer: JobObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn submit(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        self.submit_input(JobInput::Pcm(request))
    }

    pub fn submit_file(
        &self,
        path: PathBuf,
        language: String,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        self.submit_input(JobInput::File { path, language })
    }

    fn submit_input(&self, input: JobInput) -> Result<TranscriptionJob, TranscriptionServiceError> {
        let activity = crate::application::updates::activity::lease().map_err(|_| {
            TranscriptionServiceError::Busy("application update is installing".into())
        })?;
        let _configuration =
            crate::application::capture_configuration::begin_capture().map_err(|_| {
                TranscriptionServiceError::Busy("speech configuration is changing".into())
            })?;
        let mut state = self.state.lock();
        if state.pending.len() >= self.capacity {
            return Err(TranscriptionServiceError::Busy(
                "transcription queue is full".into(),
            ));
        }
        // Stable across lazy runtime/API restarts and durable history.
        let id = format!("tr_{}", uuid::Uuid::new_v4().simple());
        let (language, name) = match &input {
            JobInput::Pcm(request) => (request.language.clone(), None),
            JobInput::File { path, language } => (
                language.clone(),
                path.file_name().map(|n| n.to_string_lossy().into_owned()),
            ),
        };
        let mut job = TranscriptionJob::queued(id.clone(), language);
        job.file_name = name;
        job.progress = Some(0.0);
        state.pending.push_back(id.clone());
        state.work.insert(
            id.clone(),
            QueuedJob {
                input: Some(input),
                cancellation: OperationCancellation::default(),
                _activity: activity,
            },
        );
        state.jobs.insert(id, job.clone());
        drop(state);
        self.notify(&job);
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Option<TranscriptionJob> {
        self.state.lock().jobs.get(id).cloned()
    }

    pub fn cancel(&self, id: &str) -> Option<TranscriptionJob> {
        let mut state = self.state.lock();
        let cancellation = state.work.get(id)?.cancellation.clone();
        cancellation.cancel();
        state.pending.retain(|pending| pending != id);
        let job = state.jobs.get_mut(id)?;
        if !job.is_terminal() {
            job.state = JobState::Cancelled;
            job.finished_at_ms = Some(now_epoch_ms());
            job.error = Some(TranscriptionServiceError::Cancelled("job cancelled".into()));
        }
        let cancelled = job.clone();
        let retired = if state.work.get(id).is_some_and(|work| work.input.is_some()) {
            state.work.remove(id)
        } else {
            None
        };
        prune_terminal_jobs(&mut state);
        drop(state);
        self.notify(&cancelled);
        drop(retired); // Keep cancellation persistence admitted, outside the queue lock.
        Some(cancelled)
    }

    pub fn cancel_all(&self) {
        let mut state = self.state.lock();
        state.pending.clear();
        let active_ids: Vec<_> = state.work.keys().cloned().collect();
        let mut cancelled_jobs = Vec::new();
        for id in active_ids {
            if let Some(work) = state.work.get(&id) {
                work.cancellation.cancel();
            }
            if let Some(job) = state.jobs.get_mut(&id) {
                if !job.is_terminal() {
                    job.state = JobState::Cancelled;
                    job.finished_at_ms = Some(now_epoch_ms());
                    job.error = Some(TranscriptionServiceError::Cancelled(
                        "service shutdown".into(),
                    ));
                    cancelled_jobs.push(job.clone());
                }
            }
        }
        let pending: Vec<_> = state
            .work
            .iter()
            .filter_map(|(id, work)| work.input.is_some().then_some(id.clone()))
            .collect();
        let retired: Vec<_> = pending
            .iter()
            .filter_map(|id| state.work.remove(id))
            .collect();
        prune_terminal_jobs(&mut state);
        drop(state);
        for job in cancelled_jobs {
            self.notify(&job);
        }
        drop(retired);
    }

    /// Runs at most one queued job. The service host calls this from its worker
    /// loop; interactive dictation deliberately leaves the batch job queued.
    pub fn run_next(&self) -> Option<TranscriptionJob> {
        let _activity = crate::application::updates::activity::lease().ok()?;
        let configuration = crate::application::capture_configuration::begin_capture().ok()?;
        if self.interactive.is_active() {
            return None;
        }
        let (id, input, cancellation, preparing) = {
            let mut state = self.state.lock();
            let id = state.pending.pop_front()?;
            let (input, cancellation) = {
                let work = state.work.get_mut(&id)?;
                (work.input.take()?, work.cancellation.clone())
            };
            let job = state.jobs.get_mut(&id)?;
            if job.state == JobState::Cancelled {
                return Some(job.clone());
            }
            job.state = JobState::Preparing;
            job.started_at_ms = Some(now_epoch_ms());
            (id, input, cancellation, job.clone())
        };
        drop(configuration);
        self.notify(&preparing);
        let outcome = (|| {
            let request = match input {
                JobInput::Pcm(request) => request,
                JobInput::File { path, language } => {
                    let audio = super::audio_ingest::decode_file_cancellable(
                        &path,
                        super::audio_ingest::AudioIngestPolicy::default(),
                        &cancellation,
                    )
                    .map_err(|error| {
                        if cancellation.is_cancelled() {
                            TranscriptionServiceError::Cancelled(
                                "file preparation cancelled".into(),
                            )
                        } else {
                            TranscriptionServiceError::InvalidRequest(
                                super::audio_ingest::public_error(&error).into(),
                            )
                        }
                    })?;
                    audio.into_transcription_request(language, None)
                }
            };
            if cancellation.is_cancelled() {
                return Err(TranscriptionServiceError::Cancelled("job cancelled".into()));
            }
            let transcribing = {
                let mut state = self.state.lock();
                let job = state.jobs.get_mut(&id).expect("active job retained");
                if job.is_terminal() {
                    return Err(TranscriptionServiceError::Cancelled("job cancelled".into()));
                }
                job.state = JobState::Transcribing;
                job.clone()
            };
            self.notify(&transcribing);
            self.service
                .transcribe_with_progress(request, cancellation, &|fraction| {
                    let changed = {
                        let mut state = self.state.lock();
                        state
                            .jobs
                            .get_mut(&id)
                            .filter(|job| !job.is_terminal())
                            .and_then(|job| {
                                // Native events are bounded to percentage changes/window checkpoints.
                                let fraction = (fraction.clamp(0.0, 0.99) * 100.0).floor() / 100.0;
                                if job.progress.is_some_and(|old| old >= fraction) {
                                    return None;
                                }
                                job.progress = Some(fraction);
                                Some(job.clone())
                            })
                    };
                    if let Some(job) = changed {
                        self.notify(&job);
                    }
                })
        })();
        let mut state = self.state.lock();
        let job = state.jobs.get_mut(&id)?;
        if job.state == JobState::Cancelled {
            let cancelled = job.clone();
            let retired = state.work.remove(&id);
            drop(state);
            drop(retired);
            return Some(cancelled);
        }
        match outcome {
            Ok(result) => {
                job.state = JobState::Completed;
                job.progress = Some(1.0);
                job.result = Some(result);
            }
            Err(TranscriptionServiceError::Cancelled(message)) => {
                job.state = JobState::Cancelled;
                job.error = Some(TranscriptionServiceError::Cancelled(message));
            }
            Err(error) => {
                job.state = JobState::Failed;
                job.error = Some(error);
            }
        }
        job.finished_at_ms = Some(now_epoch_ms());
        let completed = job.clone();
        let retired = state.work.remove(&id);
        prune_terminal_jobs(&mut state);
        drop(state);
        drop(retired);
        self.notify(&completed);
        Some(completed)
    }

    pub fn snapshot(&self) -> TranscriptionQueueSnapshot {
        let state = self.state.lock();
        let mut snapshot = TranscriptionQueueSnapshot {
            capacity: self.capacity,
            jobs: state.jobs.values().rev().cloned().collect(),
            ..TranscriptionQueueSnapshot::default()
        };
        for job in state.jobs.values() {
            match job.state {
                JobState::Queued => snapshot.queued += 1,
                JobState::Preparing => snapshot.preparing += 1,
                JobState::Transcribing => snapshot.transcribing += 1,
                JobState::Completed => snapshot.completed += 1,
                JobState::Failed => snapshot.failed += 1,
                JobState::Cancelled => snapshot.cancelled += 1,
            }
        }
        snapshot
    }

    fn notify(&self, job: &TranscriptionJob) {
        if let Some(observer) = &self.observer {
            observer(job.clone());
        }
    }
}

impl<R, G> TranscriptionJobs for TranscriptionJobQueue<R, G>
where
    R: TranscriptionRuntime,
    G: InteractiveActivity,
{
    fn submit_job(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        self.submit(request)
    }
    fn submit_file(
        &self,
        path: PathBuf,
        language: String,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        self.submit_file(path, language)
    }
    fn get_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.get(id)
    }
    fn cancel_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.cancel(id)
    }
    fn run_next_job(&self) -> Option<TranscriptionJob> {
        self.run_next()
    }
    fn cancel_all_jobs(&self) {
        self.cancel_all();
    }
    fn snapshot(&self) -> TranscriptionQueueSnapshot {
        self.snapshot()
    }
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn prune_terminal_jobs(state: &mut QueueState) {
    let mut terminal_ids: Vec<_> = state
        .jobs
        .iter()
        .filter_map(|(id, job)| {
            (job.is_terminal() && !state.work.contains_key(id)).then_some(id.clone())
        })
        .collect();
    terminal_ids.sort_by_key(|id| {
        state
            .jobs
            .get(id)
            .map(|j| (j.created_at_ms, j.finished_at_ms))
    });
    let excess = terminal_ids
        .len()
        .saturating_sub(MAX_RETAINED_TERMINAL_JOBS);
    for id in terminal_ids.into_iter().take(excess) {
        state.jobs.remove(&id);
        state.work.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::error::{AppError, AppResult};
    use crate::types::Transcript;

    struct Runtime {
        calls: AtomicUsize,
    }

    impl TranscriptionRuntime for Runtime {
        fn configured_model(&self) -> AppResult<String> {
            Ok("base".into())
        }

        fn transcribe(
            &self,
            _: &[i16],
            _: &str,
            cancellation: OperationCancellation,
        ) -> AppResult<Transcript> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled("cancelled".into()));
            }
            Ok(Transcript {
                text: "ok".into(),
                detected_language: None,
                transcribe_secs: Some(0.1),
                audio_secs: Some(0.1),
                device: Some("CPU".into()),
            })
        }
    }

    struct Gate(AtomicBool);
    impl InteractiveActivity for Gate {
        fn is_active(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
    }

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            pcm_samples: vec![1],
            language: "auto".into(),
            model: Some("base".into()),
        }
    }

    #[test]
    fn file_preparation_reuses_decoder_and_cancelled_pending_paths_are_never_opened() {
        let queue = TranscriptionJobQueue::new(
            TranscriptionService::new(Runtime {
                calls: AtomicUsize::new(0),
            }),
            NoInteractiveActivity,
            4,
        );
        let missing = queue
            .submit_file(PathBuf::from("private-folder/not-there.wav"), "auto".into())
            .unwrap();
        assert!(crate::application::updates::activity::is_busy());
        queue.cancel(&missing.id).unwrap();
        assert!(queue.run_next().is_none());
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio/tone-16k-mono.wav");
        let job = queue.submit_file(fixture, "ru".into()).unwrap();
        assert_eq!(job.file_name.as_deref(), Some("tone-16k-mono.wav"));
        let result = queue.run_next().unwrap();
        assert_eq!(result.state, JobState::Completed);
        assert_eq!(result.progress, Some(1.0));
        assert_eq!(queue.service.runtime().calls.load(Ordering::Relaxed), 1);
        assert!(queue.state.lock().work.is_empty());
    }

    #[test]
    fn cancel_and_shutdown_keep_actual_work_admitted_until_native_return() {
        struct BlockingRuntime {
            started: std::sync::mpsc::Sender<()>,
            finish: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl TranscriptionRuntime for BlockingRuntime {
            fn configured_model(&self) -> AppResult<String> {
                Ok("base".into())
            }
            fn transcribe(
                &self,
                _: &[i16],
                _: &str,
                _: OperationCancellation,
            ) -> AppResult<Transcript> {
                unreachable!()
            }
            fn transcribe_with_progress(
                &self,
                _: &[i16],
                _: &str,
                _: OperationCancellation,
                progress: &(dyn Fn(f32) + Sync),
            ) -> AppResult<(
                Transcript,
                Vec<super::super::transcription_contract::SubtitleSegment>,
            )> {
                progress(0.5);
                self.started.send(()).unwrap();
                self.finish
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Ok((
                    Transcript {
                        text: "late".into(),
                        detected_language: None,
                        transcribe_secs: None,
                        audio_secs: None,
                        device: None,
                    },
                    Vec::new(),
                ))
            }
        }
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let queue = Arc::new(TranscriptionJobQueue::new(
            TranscriptionService::new(BlockingRuntime {
                started: started_tx,
                finish: Mutex::new(finish_rx),
            }),
            NoInteractiveActivity,
            4,
        ));
        let job = queue.submit(request()).unwrap();
        let active_queue = queue.clone();
        let worker = std::thread::spawn(move || active_queue.run_next().unwrap());
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(queue.get(&job.id).unwrap().progress, Some(0.5));
        queue.cancel_all();
        assert_eq!(queue.get(&job.id).unwrap().state, JobState::Cancelled);
        assert!(crate::application::updates::activity::is_busy());
        assert!(queue.state.lock().work.contains_key(&job.id));
        finish_tx.send(()).unwrap();
        let terminal = worker.join().unwrap();
        assert_eq!(terminal.state, JobState::Cancelled);
        assert!(terminal.result.is_none());
        assert!(queue.state.lock().work.is_empty());
    }

    #[test]
    fn interactive_activity_defers_batch_work_until_it_is_idle() {
        let queue = TranscriptionJobQueue::new(
            TranscriptionService::new(Runtime {
                calls: AtomicUsize::new(0),
            }),
            Gate(AtomicBool::new(true)),
            1,
        );
        let job = queue.submit(request()).unwrap();
        assert!(queue.run_next().is_none());
        assert_eq!(queue.get(&job.id).unwrap().state, JobState::Queued);
        queue.interactive.0.store(false, Ordering::Relaxed);
        assert_eq!(queue.run_next().unwrap().state, JobState::Completed);
    }

    #[test]
    fn cancelled_queued_job_never_reaches_the_runtime() {
        let queue = TranscriptionJobQueue::new(
            TranscriptionService::new(Runtime {
                calls: AtomicUsize::new(0),
            }),
            NoInteractiveActivity,
            1,
        );
        let job = queue.submit(request()).unwrap();
        assert_eq!(queue.cancel(&job.id).unwrap().state, JobState::Cancelled);
        assert!(queue.run_next().is_none());
        assert_eq!(queue.get(&job.id).unwrap().state, JobState::Cancelled);
        assert_eq!(queue.service.runtime().calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn bounded_queue_returns_a_typed_busy_error() {
        let queue = TranscriptionJobQueue::new(
            TranscriptionService::new(Runtime {
                calls: AtomicUsize::new(0),
            }),
            NoInteractiveActivity,
            1,
        );
        queue.submit(request()).unwrap();
        assert!(matches!(
            queue.submit(request()),
            Err(TranscriptionServiceError::Busy(_))
        ));
    }

    #[test]
    fn snapshot_retains_recent_terminal_jobs_and_reports_counts() {
        let queue = TranscriptionJobQueue::new(
            TranscriptionService::new(Runtime {
                calls: AtomicUsize::new(0),
            }),
            NoInteractiveActivity,
            60,
        );

        for _ in 0..=MAX_RETAINED_TERMINAL_JOBS {
            queue.submit(request()).unwrap();
            queue.run_next().unwrap();
        }

        let snapshot = queue.snapshot();
        assert_eq!(snapshot.capacity, 60);
        assert_eq!(snapshot.completed, MAX_RETAINED_TERMINAL_JOBS);
        assert_eq!(snapshot.jobs.len(), MAX_RETAINED_TERMINAL_JOBS);
        assert!(snapshot.jobs[0].created_at_ms > 0);
        assert!(snapshot.jobs[0].started_at_ms.is_some());
        assert!(snapshot.jobs[0].finished_at_ms.is_some());
    }
}
