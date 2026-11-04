//! Speech-to-Text routing for the embedded Whisper backend and standalone workers.
//!
//! CUDA and Vulkan are compile-time whisper.cpp backends.  Keeping their workers
//! in separate processes makes runtime selection possible without linking two
//! incompatible GPU backends into the Tauri process.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fono_stt_protocol::BackendKind;
use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, AppResult, RecognitionBackend};
use crate::types::{AccelerationMode, ModelId, SttModelSelection, Transcript};
use fono_core::OperationCancellation;

mod paths;
mod recognizer;
pub mod remote;
mod routing;
mod worker;
use paths::EngineCandidate;
pub use paths::WorkerPaths;
pub mod error;
pub mod ports;
pub mod types {
    pub use fono_types::*;
}
pub mod model_pool;
pub use model_pool::{ModelLease, ModelPool, ModelPoolStatus, ModelResidency};
pub mod models;
pub mod scheduler;
pub mod segmented_transcription;
pub use ports::{
    ActivityGuard, ActivityLease, ErrorMessage, GpuPackage, GpuPackageHandle, GpuPackageSource,
    NoGpuPackages, NoopActivity, SttEnvironment,
};
use recognizer::RecognizerLoad;
pub use recognizer::{
    CancellationSupport, RecognitionOptions, RecognizerCapabilities, WindowOptions,
};

/// Probe only a package whose signature/files are already locked and verified.
/// The shared session code performs the same strict hello as inference, and
/// its supervisor retains both package guards and admission through cleanup.
pub fn probe_cuda_worker(
    package: GpuPackageHandle,
    activity: Arc<dyn ActivityGuard>,
) -> AppResult<()> {
    let _lease = activity.lease()?;
    worker::WorkerSession::probe(package.worker_path().to_owned(), Some(package), activity)
}
pub use routing::SttReadinessObserver;
use routing::{ActiveEngine, EngineState};
pub use routing::{SttHealth, SttReadiness};
mod embedded;
#[cfg(feature = "offline-stt")]
mod offline;
pub mod offline_models;
mod recovery;
mod residency;
pub use recovery::{PreparedRecoverySelection, RecoverySelection, RecoverySelectionProvider};
pub use residency::GpuResidency;
mod window;
use embedded::EmbeddedRecognizer;
pub use fono_stt_protocol::{TimedSegment, WindowTranscript};

use worker::{
    worker_cleanup_busy, MailboxHealth, WorkerCleanupStatus, WorkerCleanupSupervisor, WorkerMailbox,
};

pub struct SttEngine {
    environment: SttEnvironment,
    state: Mutex<EngineState>,
    load_gate: Mutex<()>,
    source_gate: parking_lot::RwLock<()>,
    readiness: Mutex<SttReadiness>,
    readiness_observer: Mutex<Option<SttReadinessObserver>>,
    generation: std::sync::atomic::AtomicU64,
    cleanup: Arc<WorkerCleanupSupervisor>,
    recovery: Arc<recovery::RecoveryControl>,
    #[cfg(test)]
    before_recovery_arm: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

fn source_change_busy() -> AppError {
    AppError::Busy("STT source cannot change while work is active".into())
}

impl SttEngine {
    pub fn new(environment: SttEnvironment) -> Self {
        let observer = environment.readiness.clone();
        let cleanup = Arc::new(WorkerCleanupSupervisor::new(environment.activity.clone()));
        let engine = Self {
            environment,
            state: Mutex::new(EngineState::Empty),
            load_gate: Mutex::new(()),
            source_gate: parking_lot::RwLock::new(()),
            readiness: Mutex::new(SttReadiness::Unloaded),
            readiness_observer: Mutex::new(observer.clone()),
            generation: std::sync::atomic::AtomicU64::new(0),
            cleanup,
            recovery: Arc::default(),
            #[cfg(test)]
            before_recovery_arm: Mutex::new(None),
        };
        if let Some(observer) = observer {
            observer(engine.readiness());
        }
        engine
    }

    pub fn ensure_model_loaded(
        &self,
        model: &SttModelSelection,
        acceleration: AccelerationMode,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        let paths = self.worker_paths();
        self.ensure_model_loaded_with_paths(model, acceleration, &paths, cancellation)
    }

    pub fn transcribe_segments(
        &self,
        samples: &[i16],
        segments: &[fono_audio::speech_segments::SpeechSegment],
        language: &str,
        cancellation: &OperationCancellation,
        decoding: fono_types::DecodingOptions,
    ) -> AppResult<WindowTranscript> {
        let _source = self.source_gate.try_read().ok_or_else(source_change_busy)?;
        crate::segmented_transcription::transcribe_segments(
            samples,
            segments,
            cancellation,
            |window, offset| {
                (
                    self.transcribe_window_with_options(
                        window,
                        language,
                        None,
                        0,
                        cancellation,
                        offset,
                        decoding,
                    ),
                    false,
                )
            },
        )
    }

    pub fn worker_paths(&self) -> WorkerPaths {
        let mut paths = self.environment.workers.clone();
        if let Some(path) = self.environment.speech_vad_model.as_ref() {
            paths.speech_vad = Some(path.clone());
        }
        if let Some(package) = self.environment.gpu_packages.current() {
            paths = paths.with_cuda_package(package);
        }
        paths
    }

    fn error_message(&self, error: &AppError) -> String {
        (self.environment.error_message)(error)
    }

    pub fn ensure_loaded(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
    ) -> AppResult<()> {
        self.ensure_loaded_if(model_path, acceleration, worker_paths, None)
    }

    /// Typed selection entry point for recognizers. Legacy path callers remain
    /// Whisper-compatible; unavailable engines never enter the Whisper loader.
    pub fn ensure_model_loaded_with_paths(
        &self,
        model: &SttModelSelection,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        self.ensure_selection_loaded_if(model, acceleration, worker_paths, cancellation, None)
    }

    fn ensure_loaded_if(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
        valid: Option<recovery::LoadFence<'_>>,
    ) -> AppResult<()> {
        let model = SttModelSelection::from_whisper_path(model_path.to_string_lossy().into_owned());
        self.ensure_selection_loaded_if(
            &model,
            acceleration,
            worker_paths,
            &OperationCancellation::default(),
            valid,
        )
    }

    fn ensure_selection_loaded_if(
        &self,
        model: &SttModelSelection,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
        cancellation: &OperationCancellation,
        valid: Option<recovery::LoadFence<'_>>,
    ) -> AppResult<()> {
        recognizer::check_cancellation(cancellation)?;
        validate_model_selection(model)?;
        let model_path = Path::new(&model.local_path);
        let _activity = self.environment.activity.lease()?;
        let _load_gate = self.load_gate.lock();
        let _source_change = if self.is_remote() {
            let guard = self
                .source_gate
                .try_write()
                .ok_or_else(source_change_busy)?;
            if !residency::idle_for_pool(&self.state.lock()) {
                return Err(source_change_busy());
            }
            Some(guard)
        } else {
            None
        };
        if self.cleanup.is_pending() {
            return Err(worker_cleanup_busy());
        }
        if !model_path.exists() {
            tracing::warn!(path = %model_path.display(), "STT model file not found");
            let error = AppError::ModelFileMissing;
            self.load_readiness(
                SttReadiness::Failed {
                    message: self.error_message(&error),
                },
                valid,
                valid.map_or(0, |fence| fence.generation),
            )?;
            return Err(error);
        }
        let model_path_string = model_path.to_string_lossy().to_string();
        let candidates = if model.id.engine == crate::types::SttEngine::Whisper {
            worker_paths.candidates(acceleration)
        } else {
            #[cfg(feature = "offline-stt")]
            {
                match acceleration {
                    AccelerationMode::Auto | AccelerationMode::Cpu => {
                        vec![EngineCandidate::Offline]
                    }
                    AccelerationMode::Cuda | AccelerationMode::Vulkan => Vec::new(),
                }
            }
            #[cfg(not(feature = "offline-stt"))]
            {
                Vec::new()
            }
        };
        if candidates.is_empty() {
            let error = AppError::BackendUnavailable(error_backend(acceleration));
            self.load_readiness(
                SttReadiness::Failed {
                    message: self.error_message(&error),
                },
                valid,
                valid.map_or(0, |fence| fence.generation),
            )?;
            return Err(error);
        }

        // Loading an embedded model or starting a worker can take seconds.
        // Serialize replacements separately, but do not hold the routing lock:
        // ongoing transcription and device/status reads keep using the previous
        // engine until the replacement is fully prepared.
        recognizer::check_cancellation(cancellation)?;
        if valid.is_some_and(|fence| !(fence.valid)(fence.generation)) {
            return Err(AppError::Cancelled(
                "STT recovery generation or selection changed".into(),
            ));
        }
        if self.cleanup.is_pending() {
            return Err(worker_cleanup_busy());
        }
        {
            // Reusing an underlying CUDA candidate for Auto→CUDA would leave
            // its crash observer bound to the old configured acceleration.
            // Explicit selection changes prepare a new generation; ordinary
            // requests with the exact selection retain the compatible runtime.
            let exact_selection = self
                .prepared_recovery_selection(&RecoverySelection {
                    model_path: model_path.to_owned(),
                    acceleration,
                    worker_paths: worker_paths.clone(),
                })
                .is_some();
            let compatible_device = {
                let state = self.state.lock();
                (exact_selection
                    && state.model() == Some(&model.id)
                    && candidates
                        .iter()
                        .any(|candidate| state.compatible(&model_path_string, candidate)))
                .then(|| state.device())
            };
            if let Some(device) = compatible_device {
                self.load_readiness(
                    SttReadiness::Ready { device },
                    valid,
                    valid.map_or(0, |fence| fence.generation),
                )?;
                return Ok(());
            }
        }
        self.load_readiness(
            SttReadiness::Loading,
            valid,
            valid.map_or(0, |fence| fence.generation),
        )?;

        let mut errors = Vec::new();
        for candidate in candidates {
            if self.cleanup.is_pending() {
                return Err(self.pending_load_error(valid));
            }
            let loaded = recognizer::load(RecognizerLoad {
                model,
                candidate: &candidate,
                cleanup: Arc::clone(&self.cleanup),
                cancellation,
                #[cfg(feature = "offline-stt")]
                speech_vad: worker_paths.speech_vad.as_deref(),
            });
            match loaded {
                Ok(loaded) => {
                    let publication = self.recovery_publication();
                    if let Err(error) = recognizer::check_cancellation(cancellation) {
                        drop(publication);
                        drop(loaded);
                        self.load_readiness(
                            SttReadiness::Failed {
                                message: self.error_message(&error),
                            },
                            valid,
                            valid.map_or(0, |fence| fence.generation),
                        )?;
                        return Err(error);
                    }
                    if valid.is_some_and(|fence| !(fence.valid)(fence.generation)) {
                        drop(publication);
                        drop(loaded);
                        return Err(AppError::Cancelled(
                            "STT recovery became stale while loading".into(),
                        ));
                    }
                    tracing::info!("STT backend selected: {}", loaded.device());
                    let device = loaded.device();
                    let previous = {
                        let mut state = self.state.lock();
                        std::mem::replace(&mut *state, loaded)
                    };
                    let generation = self
                        .generation
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                        + 1;
                    self.arm_worker_recovery(
                        generation,
                        RecoverySelection {
                            model_path: model_path.to_owned(),
                            acceleration,
                            worker_paths: worker_paths.clone(),
                        },
                    );
                    // Arm while publication is fenced: a concurrent health
                    // failure cannot retire the new mailbox before its latched
                    // transport fault has a recovery observer.
                    drop(publication);
                    // Worker shutdown and native model cleanup may block. They
                    // must happen after the routing state lock is released.
                    drop(previous);
                    if self.cleanup.is_pending() {
                        return Err(self.pending_load_error(valid.map(|fence| {
                            recovery::LoadFence {
                                generation,
                                ..fence
                            }
                        })));
                    }
                    self.load_readiness(
                        SttReadiness::Ready {
                            device: device.to_string(),
                        },
                        valid,
                        generation,
                    )?;
                    return Ok(());
                }
                Err(error) => {
                    #[cfg(test)]
                    eprintln!("STT test backend load error: {error:?}");
                    if matches!(error, AppError::Cancelled(_)) {
                        self.load_readiness(
                            SttReadiness::Failed {
                                message: self.error_message(&error),
                            },
                            valid,
                            valid.map_or(0, |fence| fence.generation),
                        )?;
                        return Err(error);
                    }
                    if self.cleanup.is_pending() {
                        return Err(self.pending_load_error(valid));
                    }
                    errors.push(error.to_string());
                    if acceleration != AccelerationMode::Auto {
                        break;
                    }
                }
            }
        }
        tracing::error!(backend = acceleration_name(acceleration), errors = ?errors,
            "STT backend could not start");
        let error = AppError::BackendStartFailed(error_backend(acceleration));
        self.load_readiness(
            SttReadiness::Failed {
                message: self.error_message(&error),
            },
            valid,
            valid.map_or(0, |fence| fence.generation),
        )?;
        Err(error)
    }

    pub fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_internal(
            samples,
            language,
            None,
            fono_stt_protocol::DecodingOptions::default(),
            &[],
        )
    }

    pub fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_internal(
            samples,
            language,
            Some(&cancellation),
            fono_stt_protocol::DecodingOptions::default(),
            &[],
        )
    }

    pub fn transcribe_with_options(
        &self,
        samples: &[i16],
        language: &str,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.transcribe_internal(samples, language, None, decoding, &[])
    }

    pub fn transcribe_cancellable_with_options(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
    ) -> AppResult<Transcript> {
        self.transcribe_internal(samples, language, Some(&cancellation), decoding, &[])
    }

    pub fn transcribe_cancellable_with_glossary(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<Transcript> {
        self.transcribe_internal(samples, language, Some(&cancellation), decoding, glossary)
    }

    fn transcribe_internal(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: Option<&OperationCancellation>,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<Transcript> {
        let _source = self.source_gate.try_read().ok_or_else(source_change_busy)?;
        decoding.validate().map_err(AppError::Config)?;
        if self.cleanup.is_pending() {
            return Err(worker_cleanup_busy());
        }
        // Select the current session under the short-lived routing lock, then
        // run the expensive inference behind the selected engine's own lock.
        // Consequently device/status readers and a future supervisor mailbox
        // do not wait for a complete Whisper/worker request while holding the
        // global engine-state mutex.
        let active = self.state.lock().active().ok_or(AppError::ModelNotLoaded)?;
        let default_cancellation = OperationCancellation::default();
        let result = recognizer::transcribe_with_activity(
            self.environment.activity.as_ref(),
            active.recognizer(),
            samples,
            RecognitionOptions {
                language,
                decoding,
                glossary,
            },
            cancellation.unwrap_or(&default_cancellation),
        );
        if result.is_err()
            && active.worker_faulted()
            && !matches!(result, Err(AppError::Cancelled(_)))
        {
            tracing::warn!("STT worker session failed; crash recovery has been notified");
            if let Err(error) = &result {
                self.retire_failed_active(&active, self.error_message(error));
            }
        }
        result
    }

    pub fn device(&self) -> String {
        self.state.lock().device()
    }

    pub fn model(&self) -> Option<ModelId> {
        self.state.lock().model().cloned()
    }

    pub fn capabilities(&self) -> Option<RecognizerCapabilities> {
        self.state
            .lock()
            .active()
            .map(|active| active.recognizer().capabilities())
    }

    pub fn is_loaded(&self) -> bool {
        self.reap_finished_worker();
        self.cleanup.is_pending() || self.state.lock().active().is_some()
    }

    pub fn readiness(&self) -> SttReadiness {
        self.readiness.lock().clone()
    }

    /// An idle configuration transaction failed after replacing its runtime,
    /// and restoring the previous selection also failed. Retire that
    /// unpublished engine rather than leaving it usable behind an old pointer.
    pub fn reject_prepared_selection(&self, message: &str) {
        let _load = self.load_gate.lock();
        let previous = {
            let _publication = self.recovery_publication();
            let previous = std::mem::replace(&mut *self.state.lock(), EngineState::Empty);
            self.generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.set_readiness(SttReadiness::Failed {
                message: message.into(),
            });
            previous
        };
        // Child/supervisor retains package guards and cleanup admission.
        drop(previous);
    }

    /// Registers a process-local sink for readiness transitions. The current
    /// value is delivered immediately so a late subscriber cannot miss it.
    pub fn set_readiness_observer(&self, observer: SttReadinessObserver) {
        *self.readiness_observer.lock() = Some(Arc::clone(&observer));
        observer(self.readiness());
    }

    pub fn health(&self) -> SttHealth {
        self.reap_finished_worker();
        if self.cleanup.is_pending() {
            return SttHealth::Busy {
                device: self.device(),
            };
        }
        let (active, device) = {
            let state = self.state.lock();
            (state.active(), state.device())
        };
        let Some(active) = active else {
            return SttHealth::Unloaded;
        };

        let health = active.health(device, &self.environment.error_message);
        if let SttHealth::Failed { message, .. } = &health {
            self.retire_failed_active(&active, message.clone());
        }
        health
    }

    fn retire_failed_active(&self, active: &ActiveEngine, message: String) -> bool {
        let previous = {
            let _publication = self.recovery_publication();
            let mut state = self.state.lock();
            if !state.contains(active) {
                return false;
            }
            let previous = std::mem::replace(&mut *state, EngineState::Empty);
            drop(state);
            // Identity and the Failed event share the same publication fence
            // as replacement/Ready; an old probe can never overwrite new Ready.
            self.set_readiness(SttReadiness::Failed { message });
            previous
        };
        drop(previous);
        true
    }

    fn set_readiness(&self, readiness: SttReadiness) {
        *self.readiness.lock() = readiness.clone();
        let observer = self.readiness_observer.lock().clone();
        if let Some(observer) = observer {
            observer(readiness);
        }
    }

    fn pending_load_error(&self, fence: Option<recovery::LoadFence<'_>>) -> AppError {
        let error = worker_cleanup_busy();
        if let Err(stale) = self.load_readiness(
            SttReadiness::Failed {
                message: self.error_message(&error),
            },
            fence,
            fence.map_or(0, |fence| fence.generation),
        ) {
            return stale;
        }
        error
    }

    fn reap_finished_worker(&self) {
        let retired = {
            let mut state = self.state.lock();
            if matches!(&*state, EngineState::Worker { session, .. } if session.is_retired()) {
                Some(std::mem::replace(&mut *state, EngineState::Empty))
            } else {
                None
            }
        };
        // Even a completed mailbox is dropped outside the routing lock.
        drop(retired);
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new(SttEnvironment::default())
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

fn error_backend(acceleration: AccelerationMode) -> RecognitionBackend {
    match acceleration {
        AccelerationMode::Auto => RecognitionBackend::Auto,
        AccelerationMode::Cuda => RecognitionBackend::Cuda,
        AccelerationMode::Vulkan => RecognitionBackend::Vulkan,
        AccelerationMode::Cpu => RecognitionBackend::Cpu,
    }
}

fn gpu_backend_compiled() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

fn validate_model_selection(model: &SttModelSelection) -> AppResult<()> {
    if model.id.engine == crate::types::SttEngine::Whisper {
        model.validate().map_err(Into::into)
    } else {
        #[cfg(feature = "offline-stt")]
        {
            offline_models::validate_selection(model)
        }
        #[cfg(not(feature = "offline-stt"))]
        {
            Err(AppError::Config(
                "Offline STT is not enabled in this build".into(),
            ))
        }
    }
}

#[cfg(all(test, feature = "offline-stt"))]
mod comparison_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_audio_ingest;

#[cfg(feature = "test-support")]
pub mod test_support {
    pub use crate::worker::{
        WorkerChild, WorkerCleanupStatus, WorkerCleanupSupervisor, WorkerSession,
    };
}
