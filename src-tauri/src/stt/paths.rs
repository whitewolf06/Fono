use super::*;

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

    pub(super) fn candidates(&self, mode: AccelerationMode) -> Vec<EngineCandidate> {
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

pub(super) enum EngineCandidate {
    Embedded { use_gpu: bool },
    Worker { backend: BackendKind, path: PathBuf },
}
