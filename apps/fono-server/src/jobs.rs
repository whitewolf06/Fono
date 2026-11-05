//! Server-owned transcription admission and the shared STT job queue.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use std::time::Instant;

use fono_audio::speech_segments::{
    SpeechSegment, SpeechSegmentPlanner, DEFAULT_MAX_SEGMENT_SAMPLES,
};
use fono_core::OperationCancellation;
use fono_service::transcription_jobs::{
    ClientInflightLease, ClientInflightLimit, JobReservation, NoCaptureGate, NoInteractiveActivity,
    PendingLease, PendingLimit, TranscriptionJob, TranscriptionJobQueue, TranscriptionJobWorker,
    TranscriptionJobs, TranscriptionQueueSnapshot,
};
use fono_service::transcription_service::{TranscriptionRuntime, TranscriptionService};
use fono_stt::error::{SttError, SttResult};
use fono_stt::scheduler::{DeviceId, DeviceScheduler, QueueClass};
use fono_stt::segmented_transcription;
use fono_stt::{ActivityGuard, ActivityLease, ModelPool, WindowTranscript};
use fono_stt_protocol::BackendKind;
use fono_types::{
    validate_request, AccelerationMode, CoreError, ErrorCode, SubtitleSegment, Transcript,
    TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
};

use crate::config::ServerConfig;
use crate::models::PreloadPlan;
use crate::recognition_options::{RecognitionOptions, RecognitionOutput, TimestampMode};

const SAMPLE_RATE_HZ: usize = 16_000;

struct ActivityState {
    accepting: bool,
    active: usize,
}

/// Counts admitted work, including queued jobs and worker cleanup. Closing
/// admission never revokes leases that an already admitted operation owns.
pub struct ServerActivity {
    inner: Arc<ActivityInner>,
    pub(crate) observability: Arc<crate::telemetry::Observability>,
}

struct ActivityInner {
    state: Mutex<ActivityState>,
    idle: Condvar,
}

impl ServerActivity {
    pub fn new() -> Arc<Self> {
        Self::with_observability(Arc::new(crate::telemetry::Observability::default()))
    }

    pub fn with_observability(observability: Arc<crate::telemetry::Observability>) -> Arc<Self> {
        Arc::new(Self {
            observability,
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

/// Explicit placement never falls back to another backend.
pub fn acceleration_for_device(device: &str) -> Result<AccelerationMode, String> {
    match device {
        "cpu" => Ok(AccelerationMode::Cpu),
        _ if crate::backends::cuda_enabled() && crate::backends::cuda_index(device).is_some() => {
            Ok(AccelerationMode::Cuda)
        }
        _ => Err("models.placement.device: backend is unavailable in this build".into()),
    }
}

fn scheduler_device(device: &str) -> SttResult<DeviceId> {
    if device == "cpu" {
        return Ok(DeviceId::Cpu);
    }
    crate::backends::cuda_index(device)
        .map(DeviceId::Cuda)
        .ok_or_else(|| SttError::Config("Invalid model device".into()))
}

#[derive(Clone)]
struct PoolTranscriptionRuntime {
    pool: Arc<ModelPool>,
    scheduler: Arc<DeviceScheduler>,
    plan: PreloadPlan,
    acceleration: AccelerationMode,
    max_audio_samples: usize,
    config: Arc<ServerConfig>,
    observability: Arc<crate::telemetry::Observability>,
}

impl PoolTranscriptionRuntime {
    fn for_model(&self, requested: Option<&str>) -> Result<Self, TranscriptionServiceError> {
        let id = requested.unwrap_or(&self.plan.id);
        if id == self.plan.id
            && self
                .pool
                .model_reservation_state(&self.plan.selection.id)
                .is_some()
        {
            return Ok(self.clone());
        }
        let mut configuration = (*self.config).clone();
        configuration.models.preload = vec![id.to_owned()];
        let plan = crate::models::resolve_preload(&configuration)
            .map_err(|_| TranscriptionServiceError::ModelNotFound("Unknown server model".into()))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                TranscriptionServiceError::ModelNotFound("Unknown server model".into())
            })?;
        let plan = if self
            .pool
            .model_reservation_state(&plan.selection.id)
            .is_none()
        {
            crate::models::resolve_installed(&configuration, id).map_err(|message| {
                if message == "Requested model is not installed" {
                    TranscriptionServiceError::ModelNotFound(message)
                } else {
                    TranscriptionServiceError::ModelNotReady(message)
                }
            })?
        } else {
            plan
        };
        let acceleration = acceleration_for_device(&plan.device)
            .map_err(TranscriptionServiceError::InvalidRequest)?;
        let mut selected = self.clone();
        selected.plan = plan;
        selected.acceleration = acceleration;
        Ok(selected)
    }

    fn transcribe_windowed(
        &self,
        pcm_samples: &[i16],
        language: &str,
        class: QueueClass,
        cancellation: &OperationCancellation,
        pending: Option<PendingLease>,
        progress: &(dyn Fn(f32) + Sync),
    ) -> Result<WindowTranscript, TranscriptionServiceError> {
        self.transcribe_windowed_with_options(
            pcm_samples,
            language,
            class,
            cancellation,
            pending,
            progress,
            &RecognitionOptions::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn transcribe_windowed_with_options(
        &self,
        pcm_samples: &[i16],
        language: &str,
        class: QueueClass,
        cancellation: &OperationCancellation,
        pending: Option<PendingLease>,
        progress: &(dyn Fn(f32) + Sync),
        options: &RecognitionOptions,
    ) -> Result<WindowTranscript, TranscriptionServiceError> {
        validate_samples(pcm_samples.len(), self.max_audio_samples)?;
        if self.plan.selection.id.engine != fono_types::SttEngine::Whisper && language != "auto" {
            let specification = fono_stt::offline_models::specification(&self.plan.selection.id)
                .map_err(service_error_from_stt)?;
            if !specification
                .languages
                .iter()
                .any(|supported| supported == language)
            {
                return Err(TranscriptionServiceError::InvalidRequest(
                    "Selected model does not support the requested language".into(),
                ));
            }
        }
        options
            .decoding
            .validate()
            .map_err(TranscriptionServiceError::InvalidRequest)?;
        fono_stt_protocol::prompt::validate(&options.glossary)
            .map_err(|message| TranscriptionServiceError::InvalidRequest(message.into()))?;
        validate_prompt(options.prompt.as_deref())?;
        let acquired_at = Instant::now();
        let model = self.pool.acquire_on_device_verified(
            &self.plan.selection,
            self.acceleration,
            crate::backends::cuda_index(&self.plan.device),
            self.plan.residency,
            0,
            cancellation,
            || {
                crate::models::resolve_installed(&self.config, &self.plan.id)
                    .map(|_| ())
                    .map_err(|message| {
                        if message == "Requested model is not installed" {
                            SttError::ModelFileMissing
                        } else {
                            SttError::ModelNotLoaded
                        }
                    })
            },
        );
        self.observability.model_acquired(acquired_at.elapsed());
        let model = model.map_err(service_error_from_stt)?;
        let capabilities = model.capabilities().ok_or_else(|| {
            TranscriptionServiceError::ModelNotReady("Model capabilities are unavailable".into())
        })?;
        validate_options(options, capabilities)?;
        let segments = fixed_windows(pcm_samples.len()).map_err(service_error_from_stt)?;
        transcribe_scheduled_observed(
            (
                &self.scheduler,
                scheduler_device(&self.plan.device).map_err(service_error_from_stt)?,
                class,
            ),
            pcm_samples,
            &segments,
            cancellation,
            pending,
            progress,
            Some(&self.observability),
            |window, offset, window_cancel| {
                model.transcribe_window_with_glossary(
                    window,
                    language,
                    options.prompt.as_deref(),
                    0,
                    window_cancel,
                    offset,
                    options.decoding,
                    &options.glossary,
                )
            },
        )
        .map_err(service_error_from_stt)
    }
}

fn validate_prompt(prompt: Option<&str>) -> Result<(), TranscriptionServiceError> {
    // Whisper's byte-level tokenizer cannot produce more text tokens than
    // input UTF-8 bytes. This conservative server-only bound prevents the
    // shared 224-token prompt builder from dropping an accepted context.
    if prompt.is_some_and(|prompt| {
        prompt.len() > fono_stt_protocol::prompt::MAX_PROMPT_TOKENS || prompt.contains('\0')
    }) {
        return Err(TranscriptionServiceError::InvalidRequest(
            "prompt is invalid or exceeds the native prompt budget".into(),
        ));
    }
    Ok(())
}

fn validate_options(
    options: &RecognitionOptions,
    capabilities: fono_stt::RecognizerCapabilities,
) -> Result<(), TranscriptionServiceError> {
    let unsupported = (!options.decoding.is_default() && !capabilities.decoding_options)
        || (!options.glossary.is_empty() && !capabilities.initial_prompt && !capabilities.hotwords)
        || (options.prompt.is_some() && !capabilities.initial_prompt)
        || (options.timestamps == TimestampMode::Words && !capabilities.token_timestamps);
    if unsupported {
        return Err(TranscriptionServiceError::InvalidRequest(
            "Selected model does not support the requested recognition options".into(),
        ));
    }
    Ok(())
}

fn fixed_windows(samples: usize) -> SttResult<Vec<SpeechSegment>> {
    let mut planner = SpeechSegmentPlanner::new(DEFAULT_MAX_SEGMENT_SAMPLES)?;
    let mut windows = Vec::new();
    while let Some(window) = planner.next_cut(samples, None)? {
        windows.push(window);
    }
    windows.extend(planner.finish(samples)?);
    Ok(windows)
}

/// The current server shares one loaded recognizer per model. Both the
/// embedded and offline engines serialize inference behind their own mutex,
/// so admitting two windows would let a batch waiter bypass an interactive
/// request at that mutex. Workers may still prepare/decode concurrently.
fn server_model_scheduler(
    configured_cpu_parallel: usize,
    cuda_devices: impl IntoIterator<Item = u32>,
) -> SttResult<Arc<DeviceScheduler>> {
    if configured_cpu_parallel == 0 {
        return Err(SttError::Config("cpu_parallel must be positive".into()));
    }
    DeviceScheduler::new(1, cuda_devices).map(Arc::new)
}

/// Keep a device slot only for the current inference window. The segmented
/// helper retries a preempted window without losing earlier committed text.
#[cfg(test)]
fn transcribe_scheduled(
    scheduling: (&Arc<DeviceScheduler>, DeviceId, QueueClass),
    pcm_samples: &[i16],
    segments: &[SpeechSegment],
    cancellation: &OperationCancellation,
    pending: Option<PendingLease>,
    progress: &(dyn Fn(f32) + Sync),
    recognize: impl FnMut(&[i16], u64, &OperationCancellation) -> SttResult<WindowTranscript>,
) -> SttResult<WindowTranscript> {
    transcribe_scheduled_observed(
        scheduling,
        pcm_samples,
        segments,
        cancellation,
        pending,
        progress,
        None,
        recognize,
    )
}

#[allow(clippy::too_many_arguments)]
fn transcribe_scheduled_observed(
    (scheduler, device, class): (&Arc<DeviceScheduler>, DeviceId, QueueClass),
    pcm_samples: &[i16],
    segments: &[SpeechSegment],
    cancellation: &OperationCancellation,
    mut pending: Option<PendingLease>,
    progress: &(dyn Fn(f32) + Sync),
    observability: Option<&crate::telemetry::Observability>,
    mut recognize: impl FnMut(&[i16], u64, &OperationCancellation) -> SttResult<WindowTranscript>,
) -> SttResult<WindowTranscript> {
    segmented_transcription::transcribe_segments(
        pcm_samples,
        segments,
        cancellation,
        |window, offset| {
            let waiting = observability.map(|observer| observer.waiting(class));
            let permit = match scheduler.acquire(device, class, cancellation) {
                Ok(permit) => permit,
                Err(error) => return (Err(error), false),
            };
            drop(waiting);
            let active = observability.map(|observer| observer.active(class));
            // The request is active once its first device window starts.
            drop(pending.take());
            let started = Instant::now();
            let result = recognize(window, offset, &permit.cancellation);
            let preempted = class == QueueClass::Batch
                && permit.cancellation.is_cancelled()
                && !cancellation.is_cancelled();
            let report_progress = result.is_ok() && !preempted;
            if let Some(observer) = observability {
                use crate::telemetry::Outcome;
                let outcome = if preempted {
                    Outcome::Preempted
                } else {
                    match &result {
                        Ok(_) => Outcome::Ok,
                        Err(error) => match CoreError::from(error).code {
                            ErrorCode::Cancelled => Outcome::Cancelled,
                            ErrorCode::WorkerFailed => Outcome::WorkerFailed,
                            _ => Outcome::Failed,
                        },
                    }
                };
                observer.inference(outcome, started.elapsed(), window.len());
            }
            // Release the inference slot before invoking queue observers.
            drop(permit);
            drop(active);
            if report_progress {
                let done = offset as usize + window.len();
                progress(done as f32 / pcm_samples.len() as f32);
            }
            (result, preempted)
        },
    )
}

fn transcript_from_window(result: &WindowTranscript) -> Transcript {
    Transcript {
        remote: None,
        text: result.text.clone(),
        detected_language: result.detected_language.clone(),
        transcribe_secs: Some(result.transcribe_secs),
        audio_secs: Some(result.audio_secs),
        device: Some(
            match result.backend {
                BackendKind::Cpu => "CPU",
                BackendKind::Cuda => "CUDA",
                BackendKind::Vulkan => "Vulkan",
            }
            .into(),
        ),
    }
}

fn subtitles_from_window(result: &WindowTranscript) -> Vec<SubtitleSegment> {
    result
        .segments
        .iter()
        .map(|segment| SubtitleSegment {
            start_ms: segment.start_sample / 16,
            end_ms: segment.end_sample / 16,
            text: segment.text.clone(),
        })
        .collect()
}

impl TranscriptionRuntime for PoolTranscriptionRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        Ok(self.plan.id.clone())
    }

    fn selected_model(&self, requested: Option<&str>) -> Result<String, TranscriptionServiceError> {
        self.for_model(requested).map(|selected| selected.plan.id)
    }

    fn transcribe_selected_with_progress(
        &self,
        model: &str,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
        let selected = self.for_model(Some(model))?;
        selected.transcribe_with_progress(pcm_samples, language, cancellation, progress)
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        self.transcribe_windowed(
            pcm_samples,
            language,
            QueueClass::Batch,
            &cancellation,
            None,
            &|_| {},
        )
        .map(|result| transcript_from_window(&result))
    }

    fn transcribe_with_progress(
        &self,
        pcm_samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
        let result = self.transcribe_windowed(
            pcm_samples,
            language,
            QueueClass::Batch,
            &cancellation,
            None,
            progress,
        )?;
        Ok((
            transcript_from_window(&result),
            subtitles_from_window(&result),
        ))
    }
}

fn service_error_from_stt(error: SttError) -> TranscriptionServiceError {
    let public = CoreError::from(&error);
    match public.code {
        ErrorCode::Busy => TranscriptionServiceError::Busy(public.message),
        ErrorCode::Cancelled => TranscriptionServiceError::Cancelled(public.message),
        ErrorCode::ModelNotFound => TranscriptionServiceError::ModelNotFound(public.message),
        ErrorCode::InvalidRequest => TranscriptionServiceError::InvalidRequest(public.message),
        ErrorCode::WorkerFailed => TranscriptionServiceError::WorkerFailed(public.message),
        ErrorCode::ModelNotReady => TranscriptionServiceError::ModelNotReady(public.message),
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

    fn submit_job_for_client(
        &self,
        client_key: &str,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        validate_samples(request.pcm_samples.len(), self.max_audio_samples)?;
        self.inner.submit_job_for_client(client_key, request)
    }

    fn reserve_for_client(
        &self,
        client_key: &str,
    ) -> Result<JobReservation, TranscriptionServiceError> {
        self.inner.reserve_for_client(client_key)
    }

    fn submit_job_reserved(
        &self,
        reservation: JobReservation,
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        validate_samples(request.pcm_samples.len(), self.max_audio_samples)?;
        self.inner.submit_job_reserved(reservation, request)
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

    fn submit_file_for_client(
        &self,
        client_key: &str,
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
        self.inner
            .submit_file_for_client(client_key, path, language)
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

/// Admission begins before HTTP audio decoding and ends after inference.
/// The pending slot is released when the first inference window starts.
pub struct RecognitionAdmission {
    _client: ClientInflightLease,
    pending: PendingLease,
}

pub struct ServerJobs {
    queue: Arc<ServerQueue>,
    jobs: Arc<dyn TranscriptionJobs>,
    runtime: PoolTranscriptionRuntime,
    activity: Arc<ServerActivity>,
    client_limit: ClientInflightLimit,
    pending_limit: PendingLimit,
    workers: Vec<TranscriptionJobWorker>,
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
        let scheduler = server_model_scheduler(
            config.queue.cpu_parallel,
            crate::backends::configured_cuda_devices(config),
        )
        .map_err(|_| "cannot initialize device scheduler")?;
        let client_limit = ClientInflightLimit::new(config.queue.max_inflight_per_token);
        let pending_limit = PendingLimit::new(config.queue.max_queued);
        let runtime = PoolTranscriptionRuntime {
            observability: Arc::clone(&activity.observability),
            pool,
            scheduler,
            plan: default_plan,
            acceleration,
            max_audio_samples,
            config: Arc::new(config.clone()),
        };
        let queue = Arc::new(
            TranscriptionJobQueue::new(
                TranscriptionService::new(runtime.clone()),
                NoInteractiveActivity,
                config.queue.max_queued,
                activity.clone(),
                Arc::new(NoCaptureGate),
            )
            .with_client_limit(client_limit.clone())
            .with_pending_limit(pending_limit.clone()),
        );
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(LimitedJobs {
            inner: queue.clone(),
            max_audio_samples,
            max_file_bytes,
        });
        let mut workers = Vec::with_capacity(config.queue.cpu_parallel);
        for _ in 0..config.queue.cpu_parallel {
            match TranscriptionJobWorker::start(jobs.clone()) {
                Ok(worker) => workers.push(worker),
                Err(_) => {
                    for worker in &workers {
                        worker.shutdown();
                    }
                    for worker in &workers {
                        let _ = worker.join();
                    }
                    return Err("cannot start transcription job worker".into());
                }
            }
        }
        Ok(Self {
            queue,
            jobs,
            runtime,
            activity,
            client_limit,
            pending_limit,
            workers,
        })
    }

    pub fn jobs(&self) -> Arc<dyn TranscriptionJobs> {
        Arc::clone(&self.jobs)
    }

    pub fn queued_interactive(&self) -> usize {
        std::iter::once(DeviceId::Cpu)
            .chain(
                crate::backends::configured_cuda_devices(&self.runtime.config)
                    .into_iter()
                    .map(DeviceId::Cuda),
            )
            .filter_map(|device| self.runtime.scheduler.status(device))
            .map(|status| status.queued_interactive)
            .sum()
    }

    pub fn admit_client(
        &self,
        client_key: &str,
    ) -> Result<RecognitionAdmission, TranscriptionServiceError> {
        let client = self.client_limit.try_acquire(client_key)?;
        let pending = self.pending_limit.try_acquire()?;
        Ok(RecognitionAdmission {
            _client: client,
            pending,
        })
    }

    /// Synchronous server recognition. The caller must run it on a blocking
    /// thread and keep its cancellation token alive through disconnects.
    pub fn recognize(
        &self,
        client_key: &str,
        request: TranscriptionRequest,
        class: QueueClass,
        cancellation: OperationCancellation,
    ) -> Result<TranscriptionResult, TranscriptionServiceError> {
        let admission = self.admit_client(client_key)?;
        self.recognize_admitted(request, class, cancellation, admission)
    }

    /// The caller may move `admission` into a blocking decode task. Its token
    /// lease remains owned until native inference and cleanup have returned.
    pub fn recognize_admitted(
        &self,
        request: TranscriptionRequest,
        class: QueueClass,
        cancellation: OperationCancellation,
        admission: RecognitionAdmission,
    ) -> Result<TranscriptionResult, TranscriptionServiceError> {
        self.recognize_admitted_with_options(
            request,
            class,
            RecognitionOptions {
                timestamps: TimestampMode::Segments,
                ..Default::default()
            },
            cancellation,
            admission,
        )
        .map(|output| output.result)
    }

    pub fn recognize_admitted_with_options(
        &self,
        request: TranscriptionRequest,
        class: QueueClass,
        options: RecognitionOptions,
        cancellation: OperationCancellation,
        admission: RecognitionAdmission,
    ) -> Result<RecognitionOutput, TranscriptionServiceError> {
        let RecognitionAdmission { _client, pending } = admission;
        validate_request(&request)?;
        validate_samples(request.pcm_samples.len(), self.runtime.max_audio_samples)?;
        let _activity = self.activity.lease().map_err(service_error_from_stt)?;
        let runtime = self.runtime.for_model(request.model.as_deref())?;
        let result = runtime.transcribe_windowed_with_options(
            &request.pcm_samples,
            &request.language,
            class,
            &cancellation,
            Some(pending),
            &|_| {},
            &options,
        )?;
        let mut response = TranscriptionResult::from_transcript(
            transcript_from_window(&result),
            runtime.plan.id.clone(),
        );
        if options.timestamps != TimestampMode::None {
            response.segments = subtitles_from_window(&result);
        }
        let words = if options.timestamps == TimestampMode::Words {
            result
                .words
                .iter()
                .map(|word| SubtitleSegment {
                    start_ms: word.start_sample / 16,
                    end_ms: word.end_sample / 16,
                    text: word.text.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(RecognitionOutput {
            result: response,
            words,
            device: Some(runtime.plan.device),
        })
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
        for worker in &self.workers {
            worker.shutdown();
        }
    }

    /// Join on a blocking thread; it can wait for native recognition cleanup.
    pub fn join_worker(&self) -> std::thread::Result<()> {
        let mut first_failure = None;
        for worker in &self.workers {
            if let Err(error) = worker.join() {
                first_failure.get_or_insert(error);
            }
        }
        first_failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fono_stt::SttEnvironment;
    use fono_types::{ModelId, SttEngine, SttModelSelection};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    fn sample_config() -> ServerConfig {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/server.toml.example");
        crate::config::load_and_validate_with_env(
            &path,
            std::iter::empty::<(std::ffi::OsString, std::ffi::OsString)>(),
        )
        .unwrap()
    }

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
            observability: Arc::default(),
            scheduler: Arc::new(DeviceScheduler::new(1, []).unwrap()),
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
            config: Arc::new(sample_config()),
        };
        assert!(matches!(
            runtime.transcribe(&[0, 1], "auto", OperationCancellation::default()),
            Err(TranscriptionServiceError::InvalidRequest(_))
        ));
        assert_eq!(pool.status().loaded, 0);
        assert_eq!(pool.status().loading, 0);
    }

    #[test]
    fn unavailable_requested_model_fails_before_inference_or_default_fallback() {
        let pool = Arc::new(ModelPool::new(SttEnvironment::default(), 1).unwrap());
        let mut config = sample_config();
        config.models.dir =
            std::env::temp_dir().join(format!("fono-missing-model-{}", uuid::Uuid::new_v4()));
        let mut selected_config = config.clone();
        selected_config.models.preload = vec![config.models.default.clone()];
        let default_plan = crate::models::resolve_preload(&selected_config)
            .unwrap()
            .remove(0);
        let runtime = PoolTranscriptionRuntime {
            pool: Arc::clone(&pool),
            observability: Arc::default(),
            scheduler: Arc::new(DeviceScheduler::new(1, []).unwrap()),
            plan: default_plan,
            acceleration: AccelerationMode::Cpu,
            max_audio_samples: 16_000,
            config: Arc::new(config),
        };
        for id in [
            "whisper:tiny",
            "whisper:not-a-managed-model",
            "../model.bin",
        ] {
            assert!(
                matches!(
                    runtime.for_model(Some(id)),
                    Err(TranscriptionServiceError::ModelNotFound(_))
                ),
                "{id}"
            );
        }
        assert_eq!(pool.status().loaded, 0);
        assert_eq!(pool.status().loading, 0);
    }

    #[test]
    fn advanced_options_fail_closed_when_selected_engine_lacks_capability() {
        let unsupported = fono_stt::RecognizerCapabilities {
            max_segment_samples: 30 * 16_000,
            window_transcription: true,
            initial_prompt: false,
            hotwords: false,
            language_detection: true,
            automatic_language: true,
            token_timestamps: false,
            decoding_options: false,
            cancellation: fono_stt::CancellationSupport::Cooperative,
        };
        assert!(validate_options(&RecognitionOptions::default(), unsupported).is_ok());
        for options in [
            RecognitionOptions {
                glossary: vec!["Fono".into()],
                ..Default::default()
            },
            RecognitionOptions {
                prompt: Some("Context".into()),
                ..Default::default()
            },
            RecognitionOptions {
                timestamps: TimestampMode::Words,
                ..Default::default()
            },
            RecognitionOptions {
                decoding: fono_types::DecodingOptions {
                    strategy: fono_types::DecodingStrategy::Accurate,
                    threads: None,
                },
                ..Default::default()
            },
        ] {
            assert!(matches!(
                validate_options(&options, unsupported),
                Err(TranscriptionServiceError::InvalidRequest(_))
            ));
        }
        let supported = fono_stt::RecognizerCapabilities {
            initial_prompt: true,
            token_timestamps: true,
            decoding_options: true,
            ..unsupported
        };
        assert!(validate_options(
            &RecognitionOptions {
                glossary: vec!["Fono".into()],
                prompt: Some("Context".into()),
                timestamps: TimestampMode::Words,
                decoding: fono_types::DecodingOptions {
                    strategy: fono_types::DecodingStrategy::Accurate,
                    threads: None
                },
            },
            supported
        )
        .is_ok());
    }

    #[test]
    fn prompt_utf8_byte_bound_stays_within_the_native_token_budget() {
        let limit = fono_stt_protocol::prompt::MAX_PROMPT_TOKENS;
        assert!(validate_prompt(None).is_ok());
        assert!(validate_prompt(Some(&"x".repeat(limit))).is_ok());
        assert!(validate_prompt(Some(&"я".repeat(limit / 2))).is_ok());
        for prompt in [
            "x".repeat(limit + 1),
            "я".repeat(limit / 2 + 1),
            "x\0y".into(),
        ] {
            assert!(matches!(
                validate_prompt(Some(&prompt)),
                Err(TranscriptionServiceError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn cuda_placement_is_not_silently_mapped_to_cpu() {
        assert_eq!(acceleration_for_device("cpu"), Ok(AccelerationMode::Cpu));
        for device in ["cuda:0", "cuda:1"] {
            if crate::backends::cuda_enabled() {
                assert_eq!(acceleration_for_device(device), Ok(AccelerationMode::Cuda));
            } else {
                assert!(acceleration_for_device(device).is_err());
            }
        }
        assert!(acceleration_for_device("cuda:01").is_err());
    }

    #[test]
    fn interactive_starts_at_batch_window_boundary_and_batch_retries_checkpoint() {
        // The configured CPU worker count is two; this single model keeps one
        // inference slot to preserve interactive priority at the engine mutex.
        let scheduler = server_model_scheduler(2, []).unwrap();
        assert_eq!(scheduler.status(DeviceId::Cpu).unwrap().capacity, 1);
        let batch_scheduler = Arc::clone(&scheduler);
        let observations = Arc::new(crate::telemetry::Observability::default());
        let batch_observations = observations.clone();
        let (window_started_tx, window_started_rx) = mpsc::channel();
        let (finish_window_tx, finish_window_rx) = mpsc::channel();
        let (order_tx, order_rx) = mpsc::channel();
        let batch_order = order_tx.clone();
        let batch = thread::spawn(move || {
            let job_cancel = OperationCancellation::default();
            let segments = [
                SpeechSegment {
                    start: 0,
                    end: 4,
                    forced: true,
                },
                SpeechSegment {
                    start: 4,
                    end: 8,
                    forced: false,
                },
            ];
            let mut first_attempt = true;
            let result = transcribe_scheduled_observed(
                (&batch_scheduler, DeviceId::Cpu, QueueClass::Batch),
                &[1; 8],
                &segments,
                &job_cancel,
                None,
                &|_| {},
                Some(&batch_observations),
                |window, offset, window_cancel| {
                    if first_attempt {
                        first_attempt = false;
                        window_started_tx.send(()).unwrap();
                        finish_window_rx
                            .recv_timeout(Duration::from_secs(2))
                            .unwrap();
                        assert!(window_cancel.is_cancelled());
                        return Err(SttError::Cancelled("preempted window".into()));
                    }
                    batch_order.send(offset).unwrap();
                    Ok(WindowTranscript {
                        text: if offset == 0 { "first" } else { "second" }.into(),
                        segments: Vec::new(),
                        words: Vec::new(),
                        detected_language: Some("en".into()),
                        transcribe_secs: 0.01,
                        audio_secs: window.len() as f32 / SAMPLE_RATE_HZ as f32,
                        backend: BackendKind::Cpu,
                    })
                },
            )
            .unwrap();
            assert!(!job_cancel.is_cancelled());
            result.text
        });
        window_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();

        let interactive_scheduler = Arc::clone(&scheduler);
        let interactive = thread::spawn(move || {
            let request_cancel = OperationCancellation::default();
            let _permit = interactive_scheduler
                .acquire(DeviceId::Cpu, QueueClass::Interactive, &request_cancel)
                .unwrap();
            order_tx.send(u64::MAX).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while scheduler
            .status(DeviceId::Cpu)
            .is_some_and(|state| state.queued_interactive == 0)
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            scheduler.status(DeviceId::Cpu).unwrap().queued_interactive,
            1
        );
        finish_window_tx.send(()).unwrap();

        assert_eq!(
            order_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            u64::MAX
        );
        assert_eq!(order_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
        assert_eq!(order_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 4);
        interactive.join().unwrap();
        assert_eq!(batch.join().unwrap(), "first second");
        let mut metrics = String::new();
        observations.render(&mut metrics);
        assert!(metrics.contains("fono_stt_windows_total{outcome=\"preempted\"} 1\n"));
        assert!(metrics.contains("fono_stt_decode_seconds_count 3\n"));
        assert!(metrics.contains("fono_stt_realtime_factor_count 2\n"));
        assert!(metrics.contains("fono_stt_active{priority=\"batch\"} 0\n"));
    }

    #[test]
    fn cancelled_native_window_releases_observation_guards_and_has_no_success_rtf() {
        let scheduler = server_model_scheduler(1, []).unwrap();
        let observer = crate::telemetry::Observability::default();
        let result = transcribe_scheduled_observed(
            (&scheduler, DeviceId::Cpu, QueueClass::Interactive),
            &[1; 16_000],
            &fixed_windows(16_000).unwrap(),
            &OperationCancellation::default(),
            None,
            &|_| {},
            Some(&observer),
            |_, _, _| Err(SttError::Cancelled("private-native-error".into())),
        );
        assert!(matches!(result, Err(SttError::Cancelled(_))));
        let mut metrics = String::new();
        observer.render(&mut metrics);
        assert!(metrics.contains("fono_stt_windows_total{outcome=\"cancelled\"} 1\n"));
        assert!(metrics.contains("fono_stt_realtime_factor_count 0\n"));
        assert!(metrics.contains("fono_stt_active{priority=\"interactive\"} 0\n"));
        assert!(metrics.contains("fono_stt_waiting{priority=\"interactive\"} 0\n"));
        assert!(!metrics.contains("private-native-error"));
    }

    #[test]
    fn gpu_work_does_not_occupy_cpu_or_another_gpu_lane() {
        let scheduler = server_model_scheduler(2, [0, 1]).unwrap();
        let cancel = OperationCancellation::default();
        let gpu = scheduler
            .acquire(DeviceId::Cuda(0), QueueClass::Batch, &cancel)
            .unwrap();
        let cpu = scheduler
            .acquire(DeviceId::Cpu, QueueClass::Interactive, &cancel)
            .unwrap();
        let other = scheduler
            .acquire(DeviceId::Cuda(1), QueueClass::Batch, &cancel)
            .unwrap();
        assert!(!gpu.cancellation.is_cancelled());
        drop((gpu, cpu, other));
        assert!(scheduler.status(DeviceId::Cuda(2)).is_none());
    }

    #[test]
    fn batch_pending_blocks_interactive_until_dequeue_then_releases_on_first_window() {
        struct BlockingRuntime {
            started: mpsc::Sender<()>,
            finish: Mutex<mpsc::Receiver<()>>,
        }
        impl TranscriptionRuntime for BlockingRuntime {
            type Error = TranscriptionServiceError;

            fn configured_model(&self) -> Result<String, Self::Error> {
                Ok("test-model".into())
            }

            fn transcribe(
                &self,
                _: &[i16],
                _: &str,
                _: OperationCancellation,
            ) -> Result<Transcript, Self::Error> {
                self.started.send(()).unwrap();
                self.finish
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
                Ok(Transcript {
                    remote: None,
                    text: "batch".into(),
                    detected_language: None,
                    transcribe_secs: None,
                    audio_secs: None,
                    device: None,
                })
            }
        }

        let pending = PendingLimit::new(1);
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let queue = Arc::new(
            TranscriptionJobQueue::new(
                TranscriptionService::new(BlockingRuntime {
                    started: started_tx,
                    finish: Mutex::new(finish_rx),
                }),
                NoInteractiveActivity,
                2,
                Arc::new(fono_stt::NoopActivity),
                Arc::new(NoCaptureGate),
            )
            .with_pending_limit(pending.clone()),
        );
        queue
            .submit(TranscriptionRequest {
                pcm_samples: vec![1],
                language: "en".into(),
                model: None,
            })
            .unwrap();
        assert!(matches!(
            pending.try_acquire(),
            Err(TranscriptionServiceError::Busy(_))
        ));

        let working_queue = Arc::clone(&queue);
        let worker = thread::spawn(move || working_queue.run_next().unwrap());
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let interactive_pending = pending.try_acquire().unwrap();
        assert!(matches!(
            pending.try_acquire(),
            Err(TranscriptionServiceError::Busy(_))
        ));
        let scheduler = server_model_scheduler(2, []).unwrap();
        let segments = [SpeechSegment {
            start: 0,
            end: 4,
            forced: false,
        }];
        let cancellation = OperationCancellation::default();
        transcribe_scheduled(
            (&scheduler, DeviceId::Cpu, QueueClass::Interactive),
            &[1; 4],
            &segments,
            &cancellation,
            Some(interactive_pending),
            &|_| {},
            |window, _, _| {
                assert!(pending.try_acquire().is_ok());
                Ok(WindowTranscript {
                    text: "interactive".into(),
                    segments: Vec::new(),
                    words: Vec::new(),
                    detected_language: None,
                    transcribe_secs: 0.01,
                    audio_secs: window.len() as f32 / SAMPLE_RATE_HZ as f32,
                    backend: BackendKind::Cpu,
                })
            },
        )
        .unwrap();
        finish_tx.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap().state,
            fono_service::transcription_jobs::JobState::Completed
        );
    }
}
