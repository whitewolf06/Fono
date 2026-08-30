//! Bounded in-memory job queue for non-interactive transcription.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fono_core::OperationCancellation;
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
    pub state: JobState,
    pub created_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
    pub result: Option<TranscriptionResult>,
    pub error: Option<TranscriptionServiceError>,
}

impl TranscriptionJob {
    fn queued(id: String) -> Self {
        Self {
            id,
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
    pub fn start(jobs: Arc<dyn TranscriptionJobs>) -> Self {
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
            })
            .expect("start transcription job worker");
        Self { stopped, jobs }
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

struct QueuedJob {
    request: TranscriptionRequest,
    cancellation: OperationCancellation,
}

struct QueueState {
    jobs: BTreeMap<String, TranscriptionJob>,
    pending: VecDeque<String>,
    work: BTreeMap<String, QueuedJob>,
}

impl Default for QueueState {
    fn default() -> Self {
        Self {
            jobs: BTreeMap::new(),
            pending: VecDeque::new(),
            work: BTreeMap::new(),
        }
    }
}

pub struct TranscriptionJobQueue<R, G> {
    service: TranscriptionService<R>,
    interactive: G,
    capacity: usize,
    next_id: AtomicU64,
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
            next_id: AtomicU64::new(0),
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
        let mut state = self.state.lock().expect("job queue mutex poisoned");
        if state.pending.len() >= self.capacity {
            return Err(TranscriptionServiceError::Busy(
                "transcription queue is full".into(),
            ));
        }
        let id = format!(
            "tr_{:016x}",
            self.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        let job = TranscriptionJob::queued(id.clone());
        state.pending.push_back(id.clone());
        state.work.insert(
            id.clone(),
            QueuedJob {
                request,
                cancellation: OperationCancellation::default(),
            },
        );
        state.jobs.insert(id, job.clone());
        drop(state);
        self.notify(&job);
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Option<TranscriptionJob> {
        self.state
            .lock()
            .expect("job queue mutex poisoned")
            .jobs
            .get(id)
            .cloned()
    }

    pub fn cancel(&self, id: &str) -> Option<TranscriptionJob> {
        let mut state = self.state.lock().expect("job queue mutex poisoned");
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
        prune_terminal_jobs(&mut state);
        drop(state);
        self.notify(&cancelled);
        Some(cancelled)
    }

    pub fn cancel_all(&self) {
        let mut state = self.state.lock().expect("job queue mutex poisoned");
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
        drop(state);
        for job in cancelled_jobs {
            self.notify(&job);
        }
    }

    /// Runs at most one queued job. The service host calls this from its worker
    /// loop; interactive dictation deliberately leaves the batch job queued.
    pub fn run_next(&self) -> Option<TranscriptionJob> {
        if self.interactive.is_active() {
            return None;
        }
        let (id, request, cancellation, preparing) = {
            let mut state = self.state.lock().expect("job queue mutex poisoned");
            let id = state.pending.pop_front()?;
            let (request, cancellation) = {
                let work = state.work.get(&id)?;
                (work.request.clone(), work.cancellation.clone())
            };
            let job = state.jobs.get_mut(&id)?;
            if job.state == JobState::Cancelled {
                return Some(job.clone());
            }
            job.state = JobState::Preparing;
            job.started_at_ms = Some(now_epoch_ms());
            (id, request, cancellation, job.clone())
        };
        self.notify(&preparing);
        let transcribing = {
            let mut state = self.state.lock().expect("job queue mutex poisoned");
            let job = state.jobs.get_mut(&id)?;
            job.state = JobState::Transcribing;
            job.clone()
        };
        self.notify(&transcribing);
        let outcome = self.service.transcribe_cancellable(request, cancellation);
        let mut state = self.state.lock().expect("job queue mutex poisoned");
        let job = state.jobs.get_mut(&id)?;
        if job.state == JobState::Cancelled {
            let cancelled = job.clone();
            state.work.remove(&id);
            return Some(cancelled);
        }
        match outcome {
            Ok(result) => {
                job.state = JobState::Completed;
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
        state.work.remove(&id);
        prune_terminal_jobs(&mut state);
        drop(state);
        self.notify(&completed);
        Some(completed)
    }

    pub fn snapshot(&self) -> TranscriptionQueueSnapshot {
        let state = self.state.lock().expect("job queue mutex poisoned");
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
    let terminal_ids: Vec<_> = state
        .jobs
        .iter()
        .filter_map(|(id, job)| job.is_terminal().then_some(id.clone()))
        .collect();
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
            model: "base".into(),
        }
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
