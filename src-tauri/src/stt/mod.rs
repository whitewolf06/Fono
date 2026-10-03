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
use tauri::AppHandle;
#[cfg(not(debug_assertions))]
use tauri::Manager;

use crate::error::{AppError, AppResult};
use crate::operation::OperationCancellation;
use crate::types::{AccelerationMode, Transcript};

mod paths;
mod routing;
mod worker;
use paths::EngineCandidate;
pub use paths::{worker_paths_for_app, WorkerPaths};
use routing::{ActiveEngine, EngineState, SttReadinessObserver};
pub use routing::{SttHealth, SttReadiness};
mod embedded;
mod window;
use embedded::EmbeddedEngine;
pub use fono_stt_protocol::{TimedSegment, WindowTranscript};

use worker::{MailboxHealth, WorkerMailbox, WorkerSession};

pub struct SttEngine {
    state: Mutex<EngineState>,
    load_gate: Mutex<()>,
    readiness: Mutex<SttReadiness>,
    readiness_observer: Mutex<Option<SttReadinessObserver>>,
}

impl SttEngine {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(EngineState::Empty),
            load_gate: Mutex::new(()),
            readiness: Mutex::new(SttReadiness::Unloaded),
            readiness_observer: Mutex::new(None),
        }
    }

    pub fn ensure_loaded(
        &self,
        model_path: &Path,
        acceleration: AccelerationMode,
        worker_paths: &WorkerPaths,
    ) -> AppResult<()> {
        if !model_path.exists() {
            let error = AppError::Stt(format!("model file not found: {}", model_path.display()));
            self.set_failed_readiness(&error);
            return Err(error);
        }
        let model_path_string = model_path.to_string_lossy().to_string();
        let candidates = worker_paths.candidates(acceleration);
        if candidates.is_empty() {
            let error = AppError::Stt(format!(
                "{} backend is not packaged in this release",
                acceleration_name(acceleration)
            ));
            self.set_failed_readiness(&error);
            return Err(error);
        }

        // Loading an embedded model or starting a worker can take seconds.
        // Serialize replacements separately, but do not hold the routing lock:
        // ongoing transcription and device/status reads keep using the previous
        // engine until the replacement is fully prepared.
        let _load_gate = self.load_gate.lock();
        {
            let mut state = self.state.lock();
            if candidates
                .iter()
                .any(|candidate| state.compatible(&model_path_string, candidate))
            {
                self.set_readiness(SttReadiness::Ready {
                    device: state.device(),
                });
                return Ok(());
            }
        }
        self.set_readiness(SttReadiness::Loading);

        let mut errors = Vec::new();
        for candidate in candidates {
            let loaded = match &candidate {
                EngineCandidate::Embedded { use_gpu } => EmbeddedEngine::load(model_path, *use_gpu)
                    .map(|engine| {
                        let device = engine.device();
                        EngineState::Embedded {
                            engine: Arc::new(Mutex::new(engine)),
                            device,
                        }
                    }),
                EngineCandidate::Worker { backend, path } => {
                    WorkerSession::start(path.clone(), *backend, model_path).map(|worker| {
                        let device = worker.device();
                        EngineState::Worker {
                            session: Arc::new(WorkerMailbox::start(worker)),
                            device,
                        }
                    })
                }
            };
            match loaded {
                Ok(loaded) => {
                    tracing::info!("STT backend selected: {}", loaded.device());
                    let device = loaded.device();
                    let previous = {
                        let mut state = self.state.lock();
                        std::mem::replace(&mut *state, loaded)
                    };
                    // Worker shutdown and native model cleanup may block. They
                    // must happen after the routing state lock is released.
                    drop(previous);
                    self.set_readiness(SttReadiness::Ready {
                        device: device.to_string(),
                    });
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
        let error = AppError::Stt(format!(
            "could not start {} backend: {}",
            acceleration_name(acceleration),
            errors.join("; ")
        ));
        self.set_failed_readiness(&error);
        Err(error)
    }

    pub fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        self.transcribe_internal(samples, language, None)
    }

    pub fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe_internal(samples, language, Some(&cancellation))
    }

    fn transcribe_internal(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: Option<&OperationCancellation>,
    ) -> AppResult<Transcript> {
        // Select the current session under the short-lived routing lock, then
        // run the expensive inference behind the selected engine's own lock.
        // Consequently device/status readers and a future supervisor mailbox
        // do not wait for a complete Whisper/worker request while holding the
        // global engine-state mutex.
        let active = self.state.lock().active().ok_or(AppError::ModelNotLoaded)?;
        let result = match cancellation {
            Some(cancellation) => active.transcribe_cancellable(samples, language, cancellation),
            None => active.transcribe(samples, language),
        };
        if result.is_err() && active.is_worker() && !matches!(result, Err(AppError::Cancelled(_))) {
            tracing::warn!("STT worker session failed and will be restarted on the next operation");
            let mut state = self.state.lock();
            if state.contains(&active) {
                *state = EngineState::Empty;
            }
        }
        result
    }

    pub fn device(&self) -> String {
        self.state.lock().device()
    }

    pub fn is_loaded(&self) -> bool {
        !matches!(&*self.state.lock(), EngineState::Empty)
    }

    pub fn readiness(&self) -> SttReadiness {
        self.readiness.lock().clone()
    }

    /// Registers a process-local sink for readiness transitions. The current
    /// value is delivered immediately so a late subscriber cannot miss it.
    pub fn set_readiness_observer(&self, observer: SttReadinessObserver) {
        *self.readiness_observer.lock() = Some(Arc::clone(&observer));
        observer(self.readiness());
    }

    pub fn health(&self) -> SttHealth {
        let (active, device) = {
            let state = self.state.lock();
            (state.active(), state.device())
        };
        let Some(active) = active else {
            return SttHealth::Unloaded;
        };

        let health = active.health(device);
        if let SttHealth::Failed { message, .. } = &health {
            let mut state = self.state.lock();
            if state.contains(&active) {
                *state = EngineState::Empty;
            }
            self.set_readiness(SttReadiness::Failed {
                message: message.clone(),
            });
        }
        health
    }

    fn set_readiness(&self, readiness: SttReadiness) {
        *self.readiness.lock() = readiness.clone();
        let observer = self.readiness_observer.lock().clone();
        if let Some(observer) = observer {
            observer(readiness);
        }
    }

    fn set_failed_readiness(&self, error: &AppError) {
        self.set_readiness(SttReadiness::Failed {
            message: error.to_string(),
        });
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
mod tests;
