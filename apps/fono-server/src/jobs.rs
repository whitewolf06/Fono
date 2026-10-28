//! Server-owned transcription admission and the shared STT job queue.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use fono_core::OperationCancellation;
use fono_service::transcription_jobs::{
    NoCaptureGate, NoInteractiveActivity, TranscriptionJob, TranscriptionJobQueue,
    TranscriptionJobWorker, TranscriptionJobs, TranscriptionQueueSnapshot,
};
use fono_service::transcription_service::{TranscriptionRuntime, TranscriptionService};
use fono_stt::error::{SttError, SttResult};
use fono_stt::{ActivityGuard, ActivityLease, ModelPool};
use fono_types::{
    AccelerationMode, CoreError, ErrorCode, Transcript, TranscriptionRequest,
    TranscriptionServiceError,
};

use crate::config::ServerConfig;
use crate::models::PreloadPlan;

const SAMPLE_RATE_HZ: usize = 16_000;

struct ActivityState {
    accepting: bool,
    active: usize,
}

/// Counts admitted work, including queued jobs and worker cleanup. Closing
/// admission never revokes leases that an already admitted operation owns.
pub struct ServerActivity {
    inner: Arc<ActivityInner>,
}

struct ActivityInner {
    state: Mutex<ActivityState>,
    idle: Condvar,
}

impl ServerActivity {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(ActivityInner {
                state: Mutex::new(ActivityState {
                    accepting: true,
                    active: 0,
                }),
                idle: Condvar::new(),
            }),
        })
    }

    pub fn close(&self) {
        self.lock_state().accepting = false;
        self.inner.idle.notify_all();
    }

    pub fn active(&self) -> usize {
        self.lock_state().active
    }

    /// Wait only after the queue has retired pending jobs. A queued job owns
    /// an activity lease even though it is not currently transcribing.
    pub fn wait_zero(&self, timeout: Duration) -> bool {
        let state = self.lock_state();
        let (state, _) = self
            .inner
            .idle
            .wait_timeout_while(state, timeout, |state| state.active != 0)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active == 0
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, ActivityState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn issue_lease(&self) -> ActivityLease {
        self.lock_state().active += 1;
        ActivityLease::new(ActivityToken {
            owner: Arc::clone(&self.inner),
        })
    }
}

struct ActivityToken {
    owner: Arc<ActivityInner>,
}

impl Drop for ActivityToken {
    fn drop(&mut self) {
        let mut state = self
            .owner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        debug_assert!(state.active > 0);
        state.active -= 1;
        if state.active == 0 {
            self.owner.idle.notify_all();
        }
    }
}

impl ActivityGuard for ServerActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        let mut state = self.lock_state();
        if !state.accepting {
            return Err(SttError::Busy("server is shutting down".into()));
        }
        state.active += 1;
        drop(state);
        Ok(ActivityLease::new(ActivityToken {
            owner: Arc::clone(&self.inner),
        }))
    }

    fn cleanup_lease(&self) -> ActivityLease {
        self.issue_lease()
    }
}

/// The current server package intentionally includes only the CPU backend.
/// A requested CUDA placement must fail instead of silently using CPU.
pub fn acceleration_for_device(device: &str) -> Result<AccelerationMode, String> {
    match device {
        "cpu" => Ok(AccelerationMode::Cpu),
        _ => Err("models.placement.device: backend is unavailable in this build".into()),
    }
}

struct PoolTranscriptionRuntime {
    pool: Arc<ModelPool>,
    plan: PreloadPlan,
    acceleration: AccelerationMode,
    max_audio_samples: usize,
}

impl TranscriptionRuntime for PoolTranscriptionRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        Ok(self.plan.id.clone())
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        validate_samples(pcm_samples.len(), self.max_audio_samples)?;
        let model = self
            .pool
            .acquire(
                &self.plan.selection,
                self.acceleration,
                self.plan.residency,
                0,
                &cancellation,
            )
            .map_err(service_error_from_stt)?;
        model
            .transcribe_cancellable(pcm_samples, language, cancellation)
            .map_err(service_error_from_stt)
    }
}

fn service_error_from_stt(error: SttError) -> TranscriptionServiceError {
    let public = CoreError::from(&error);
    match public.code {
        ErrorCode::Busy => TranscriptionServiceError::Busy(public.message),
        ErrorCode::Cancelled => TranscriptionServiceError::Cancelled(public.message),
        ErrorCode::ModelNotReady | ErrorCode::ModelNotFound => {
            TranscriptionServiceError::ModelNotReady(public.message)
        }
        _ => TranscriptionServiceError::Failed(public.message),
    }
}

fn validate_samples(count: usize, max: usize) -> Result<(), TranscriptionServiceError> {
    if count > max {
        return Err(TranscriptionServiceError::InvalidRequest(
            "audio exceeds server.max_audio_seconds".into(),
        ));
    }
    Ok(())
}

struct LimitedJobs {
    inner: Arc<dyn TranscriptionJobs>,
    max_audio_samples: usize,
    max_file_bytes: u64,
}

impl TranscriptionJobs for LimitedJobs {
    fn submit_job(
        &self,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        validate_samples(request.pcm_samples.len(), self.max_audio_samples)?;
        self.inner.submit_job(request)
    }

    fn submit_file(
        &self,
        path: PathBuf,
        language: String,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        let size = std::fs::metadata(&path)
            .map_err(|_| TranscriptionServiceError::InvalidRequest("invalid audio file".into()))?
            .len();
        if size > self.max_file_bytes {
            return Err(TranscriptionServiceError::InvalidRequest(
                "audio exceeds server.max_upload_mb".into(),
            ));
        }
        self.inner.submit_file(path, language)
    }

    fn get_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.inner.get_job(id)
    }

    fn cancel_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.inner.cancel_job(id)
    }

    fn run_next_job(&self) -> Option<TranscriptionJob> {
        self.inner.run_next_job()
    }

    fn cancel_all_jobs(&self) {
        self.inner.cancel_all_jobs();
    }

    fn snapshot(&self) -> TranscriptionQueueSnapshot {
        self.inner.snapshot()
    }
}

type ServerQueue = TranscriptionJobQueue<PoolTranscriptionRuntime, NoInteractiveActivity>;

pub struct ServerJobs {
    queue: Arc<ServerQueue>,
    jobs: Arc<dyn TranscriptionJobs>,
    worker: TranscriptionJobWorker,
}

impl ServerJobs {
    pub fn new(
        pool: Arc<ModelPool>,
        default_plan: PreloadPlan,
        config: &ServerConfig,
        activity: Arc<ServerActivity>,
    ) -> Result<Self, String> {
        if default_plan.id != config.models.default {
            return Err("models.default does not match the selected runtime model".into());
        }
        u32::try_from(config.server.max_audio_seconds)
            .map_err(|_| "server.max_audio_seconds exceeds the audio decoder range")?;
        let acceleration = acceleration_for_device(&default_plan.device)?;
        let max_audio_samples = usize::try_from(config.server.max_audio_seconds)
            .ok()
            .and_then(|seconds| seconds.checked_mul(SAMPLE_RATE_HZ))
            .ok_or("server.max_audio_seconds exceeds the supported sample range")?;
        let max_file_bytes = u64::try_from(config.server.max_upload_mb)
            .ok()
            .and_then(|megabytes| megabytes.checked_mul(1024 * 1024))
            .ok_or("server.max_upload_mb exceeds the supported byte range")?;
        let runtime = PoolTranscriptionRuntime {
            pool,
            plan: default_plan,
            acceleration,
            max_audio_samples,
        };
        let queue = Arc::new(TranscriptionJobQueue::new(
            TranscriptionService::new(runtime),
            NoInteractiveActivity,
            config.queue.max_queued,
            activity,
            Arc::new(NoCaptureGate),
        ));
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(LimitedJobs {
            inner: queue.clone(),
            max_audio_samples,
            max_file_bytes,
        });
        let worker = TranscriptionJobWorker::start(jobs.clone())
            .map_err(|_| "cannot start transcription job worker".to_owned())?;
        Ok(Self {
            queue,
            jobs,
            worker,
        })
    }

    pub fn jobs(&self) -> Arc<dyn TranscriptionJobs> {
        Arc::clone(&self.jobs)
    }

    pub fn running_count(&self) -> usize {
        let snapshot = self.jobs.snapshot();
        snapshot.preparing + snapshot.transcribing
    }

    /// Cancel only jobs that never started. Call after closing admission.
    pub fn cancel_pending(&self) {
        self.queue.cancel_pending();
    }

    pub fn cancel_all(&self) {
        self.jobs.cancel_all_jobs();
    }

    pub fn shutdown_worker(&self) {
        self.worker.shutdown();
    }

    /// Join on a blocking thread; it can wait for native recognition cleanup.
    pub fn join_worker(&self) -> std::thread::Result<()> {
        self.worker.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fono_stt::SttEnvironment;
    use fono_types::{ModelId, SttEngine, SttModelSelection};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn shutdown_blocks_new_work_but_keeps_cleanup_accounted() {
        let activity = ServerActivity::new();
        let admitted = activity.lease().unwrap();
        let background = admitted.clone();
        assert_eq!(activity.active(), 1);

        activity.close();
        assert!(matches!(activity.lease(), Err(SttError::Busy(_))));
        drop(admitted);
        assert!(!activity.wait_zero(Duration::ZERO));

        let cleanup = activity.cleanup_lease();
        assert_eq!(activity.active(), 2);
        drop(background);
        assert!(!activity.wait_zero(Duration::ZERO));
        drop(cleanup);
        assert!(activity.wait_zero(Duration::ZERO));
    }

    #[derive(Default)]
    struct CountingJobs {
        submissions: AtomicUsize,
    }

    impl TranscriptionJobs for CountingJobs {
        fn submit_job(
            &self,
            _request: TranscriptionRequest,
        ) -> Result<TranscriptionJob, TranscriptionServiceError> {
            self.submissions.fetch_add(1, Ordering::Relaxed);
            Err(TranscriptionServiceError::Busy("test queue".into()))
        }

        fn get_job(&self, _id: &str) -> Option<TranscriptionJob> {
            None
        }

        fn cancel_job(&self, _id: &str) -> Option<TranscriptionJob> {
            None
        }

        fn run_next_job(&self) -> Option<TranscriptionJob> {
            None
        }

        fn cancel_all_jobs(&self) {}
    }

    #[test]
    fn oversized_pcm_is_rejected_before_queue_admission() {
        let inner = Arc::new(CountingJobs::default());
        let jobs = LimitedJobs {
            inner: inner.clone(),
            max_audio_samples: 2,
            max_file_bytes: 1,
        };
        let request = TranscriptionRequest {
            pcm_samples: vec![0, 1, 2],
            language: "auto".into(),
            model: None,
        };
        assert!(matches!(
            jobs.submit_job(request),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
        assert_eq!(inner.submissions.load(Ordering::Relaxed), 0);

        let within_limit = TranscriptionRequest {
            pcm_samples: vec![0, 1],
            language: "auto".into(),
            model: None,
        };
        assert!(matches!(
            jobs.submit_job(within_limit),
            Err(TranscriptionServiceError::Busy(_))
        ));
        assert_eq!(inner.submissions.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn decoded_audio_is_checked_before_model_acquisition() {
        let pool = Arc::new(ModelPool::new(SttEnvironment::default(), 1).unwrap());
        let runtime = PoolTranscriptionRuntime {
            pool: Arc::clone(&pool),
            plan: PreloadPlan {
                id: "whisper:base".into(),
                selection: SttModelSelection {
                    id: ModelId {
                        engine: SttEngine::Whisper,
                        variant: "base".into(),
                    },
                    local_path: "unused-model-path".into(),
                },
                device: "cpu".into(),
                residency: fono_stt::ModelResidency::Resident,
            },
            acceleration: AccelerationMode::Cpu,
            max_audio_samples: 1,
        };
        assert!(matches!(
            runtime.transcribe(&[0, 1], "auto", OperationCancellation::default()),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
        assert_eq!(pool.status().loaded, 0);
        assert_eq!(pool.status().loading, 0);
    }

    #[test]
    fn cuda_placement_is_not_silently_mapped_to_cpu() {
        assert_eq!(acceleration_for_device("cpu"), Ok(AccelerationMode::Cpu));
        assert!(acceleration_for_device("cuda:0").is_err());
        assert!(acceleration_for_device("cuda:1").is_err());
    }
}
