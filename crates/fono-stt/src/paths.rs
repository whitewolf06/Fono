use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerPaths {
    pub cuda: Option<PathBuf>,
    pub vulkan: Option<PathBuf>,
    /// Shared packaged VAD for offline segmentation; no additional runtime.
    pub speech_vad: Option<PathBuf>,
    // Keep verified file handles alive through worker start and recovery.
    pub(crate) cuda_package: Option<crate::ports::GpuPackageHandle>,
    // Protocol fixtures inject their own cmd processes, never GPU inference.
    #[cfg(test)]
    allow_fake_workers: bool,
}

impl WorkerPaths {
    pub fn with_cuda_package(mut self, package: crate::ports::GpuPackageHandle) -> Self {
        self.cuda = Some(package.worker_path().to_owned());
        self.cuda_package = Some(package);
        self
    }
    pub fn with_speech_vad(mut self, path: PathBuf) -> Self {
        self.speech_vad = Some(path);
        self
    }

    pub fn from_resource_dir(resource_dir: &Path) -> Self {
        if cfg!(feature = "cpu-only") {
            return Self::default();
        }
        // Tauri preserves a resource glob's parent directory in bundled apps,
        // while a directly launched development binary may expose the resource
        // directory itself. Support both layouts.
        let nested = resource_dir.join("resources").join("stt-workers");
        let direct = resource_dir.join("stt-workers");
        Self {
            // CUDA is installed only from a separately signed package. A
            // stale DLL/executable in bundled resources is never executable.
            cuda: None,
            speech_vad: None,
            vulkan: worker_if_present(nested.join(format!(
                "fono-stt-vulkan-worker{}",
                std::env::consts::EXE_SUFFIX
            )))
            .or_else(|| {
                worker_if_present(direct.join(format!(
                    "fono-stt-vulkan-worker{}",
                    std::env::consts::EXE_SUFFIX
                )))
            })
            .or_else(|| {
                worker_if_present(resource_dir.join(format!(
                    "resources/stt-base/fono-stt-vulkan-worker{}",
                    std::env::consts::EXE_SUFFIX
                )))
            }),
            cuda_package: None,
            #[cfg(test)]
            allow_fake_workers: false,
        }
    }

    pub fn capabilities(&self) -> (bool, bool) {
        if self.restrict_to_cpu() {
            return (false, false);
        }
        (
            self.cuda.is_some() || cfg!(feature = "cuda"),
            self.vulkan.is_some() || cfg!(feature = "vulkan"),
        )
    }

    pub(super) fn candidates(&self, mode: AccelerationMode) -> Vec<EngineCandidate> {
        if self.restrict_to_cpu() {
            return match mode {
                AccelerationMode::Auto | AccelerationMode::Cpu => {
                    vec![EngineCandidate::Embedded { use_gpu: false }]
                }
                AccelerationMode::Cuda | AccelerationMode::Vulkan => Vec::new(),
            };
        }
        let embedded_gpu = gpu_backend_compiled();
        match mode {
            AccelerationMode::Auto => {
                let mut candidates = Vec::new();
                if let Some(path) = self.cuda.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Cuda,
                        path: path.clone(),
                        package: self.cuda_package.clone().map(Box::new),
                    });
                }
                if let Some(path) = self.vulkan.as_ref() {
                    candidates.push(EngineCandidate::Worker {
                        backend: BackendKind::Vulkan,
                        path: path.clone(),
                        package: None,
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
                        package: self.cuda_package.clone().map(Box::new),
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
                        package: None,
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

    fn restrict_to_cpu(&self) -> bool {
        #[cfg(test)]
        if self.allow_fake_workers {
            return false;
        }
        cfg!(feature = "cpu-only")
    }

    /// Explicit injection for worker-protocol/process fixtures. Production
    /// discovery can never construct this test-only override.
    #[cfg(test)]
    pub(super) fn for_test_workers(cuda: Option<PathBuf>, vulkan: Option<PathBuf>) -> Self {
        Self {
            cuda,
            vulkan,
            speech_vad: None,
            cuda_package: None,
            allow_fake_workers: true,
        }
    }
}

fn worker_if_present(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

pub(super) enum EngineCandidate {
    #[cfg(feature = "offline-stt")]
    Offline,
    Embedded {
        use_gpu: bool,
    },
    Worker {
        backend: BackendKind,
        path: PathBuf,
        package: Option<Box<crate::ports::GpuPackageHandle>>,
    },
}

#[cfg(all(test, feature = "cpu-only"))]
mod cpu_tests {
    use super::*;

    #[test]
    fn cpu_profile_ignores_even_present_gpu_worker_resources() {
        let directory =
            std::env::temp_dir().join(format!("fono-cpu-paths-{}", uuid::Uuid::new_v4()));
        let workers = directory.join("resources/stt-workers");
        std::fs::create_dir_all(&workers).unwrap();
        for name in ["fono-stt-cuda-worker.exe", "fono-stt-vulkan-worker.exe"] {
            std::fs::write(workers.join(name), []).unwrap();
        }
        let found = WorkerPaths::from_resource_dir(&directory);
        assert!(found.cuda.is_none() && found.vulkan.is_none());
        std::fs::remove_file(workers.join("fono-stt-cuda-worker.exe")).unwrap();
        std::fs::remove_file(workers.join("fono-stt-vulkan-worker.exe")).unwrap();
        std::fs::remove_dir(&workers).unwrap();
        std::fs::remove_dir(directory.join("resources")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn cpu_profile_never_selects_gpu_even_with_untrusted_paths() {
        let paths = WorkerPaths {
            cuda: Some("cuda.exe".into()),
            vulkan: Some("vulkan.exe".into()),
            ..WorkerPaths::default()
        };
        assert_eq!(paths.capabilities(), (false, false));
        for mode in [AccelerationMode::Auto, AccelerationMode::Cpu] {
            assert!(matches!(
                paths.candidates(mode).as_slice(),
                [EngineCandidate::Embedded { use_gpu: false }]
            ));
        }
        assert!(paths.candidates(AccelerationMode::Cuda).is_empty());
        assert!(paths.candidates(AccelerationMode::Vulkan).is_empty());
    }
}
