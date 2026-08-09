//! Speech-to-Text routing for the embedded Whisper backend and standalone workers.
//!
//! CUDA and Vulkan are compile-time whisper.cpp backends.  Keeping their workers
//! in separate processes makes runtime selection possible without linking two
//! incompatible GPU backends into the Tauri process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fono_stt_protocol::BackendKind;
use parking_lot::Mutex;
use tauri::AppHandle;
#[cfg(not(debug_assertions))]
use tauri::Manager;
use whisper_rs::{SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{AppError, AppResult};
use crate::types::{AccelerationMode, Transcript};

mod worker;

use worker::WorkerSession;

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
                current.compatible_with(model_path, *backend, path)
            }
            _ => false,
        }
    }

    fn device(&self) -> String {
        match self {
            Self::Empty => "CPU".into(),
            Self::Embedded(current) => current.device().into(),
            Self::Worker(current) => current.device().into(),
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
}
