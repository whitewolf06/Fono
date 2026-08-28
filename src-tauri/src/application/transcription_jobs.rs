//! Bounded in-memory job queue for non-interactive transcription.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use fono_core::OperationCancellation;
use serde::Serialize;

use crate::application::transcription_contract::{
    TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
};
use crate::application::transcription_service::{TranscriptionRuntime, TranscriptionService};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Preparing,
    Transcribing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptionJob {
    pub id: String,
    pub state: JobState,
    pub result: Option<TranscriptionResult>,
    pub error: Option<TranscriptionServiceError>,
}

impl TranscriptionJob {
    fn queued(id: String) -> Self {
        Self {
            id,
            state: JobState::Queued,
            result: None,
            error: None,
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        )
    }
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
}

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
        }
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
            job.error = Some(TranscriptionServiceError::Cancelled("job cancelled".into()));
        }
        Some(job.clone())
    }

    /// Runs at most one queued job. The service host calls this from its worker
    /// loop; interactive dictation deliberately leaves the batch job queued.
    pub fn run_next(&self) -> Option<TranscriptionJob> {
        if self.interactive.is_active() {
            return None;
        }
        let (id, request, cancellation) = {
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
            (id, request, cancellation)
        };
        {
            let mut state = self.state.lock().expect("job queue mutex poisoned");
            if let Some(job) = state.jobs.get_mut(&id) {
                job.state = JobState::Transcribing;
            }
        }
        let outcome = self.service.transcribe_cancellable(request, cancellation);
        let mut state = self.state.lock().expect("job queue mutex poisoned");
        let job = state.jobs.get_mut(&id)?;
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
        let completed = job.clone();
        state.work.remove(&id);
        Some(completed)
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
        fn ensure_ready(&self) -> AppResult<()> {
            Ok(())
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
}
