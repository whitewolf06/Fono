//! Desktop resource discovery and verified GPU package adapter.
use fono_stt::WorkerPaths;
use std::path::Path;
use tauri::{AppHandle, Manager};

pub fn worker_paths_for_app(app: &AppHandle) -> WorkerPaths {
    let mut paths = bundled_worker_paths(app);
    paths.speech_vad = crate::application::speech_gate::model_path(app).ok();
    if !cfg!(feature = "cpu-only") {
        // The cache owns locked, fully verified files. It is initialized on a
        // blocking thread and replaced atomically with the durable pointer;
        // status/dictation callers never hash a 500 MiB payload on UI threads.
        if let Some(package) = app
            .try_state::<crate::application::cuda_packages::CudaPackageService>()
            .and_then(|service| service.verified_active())
        {
            paths = paths.with_cuda_package(fono_stt::ports::GpuPackageHandle::new(package));
        }
    }
    paths
}

fn bundled_worker_paths(app: &AppHandle) -> WorkerPaths {
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

impl fono_stt::ports::GpuPackage for crate::application::cuda_packages::VerifiedPackage {
    fn worker_path(&self) -> &Path {
        self.worker_path()
    }
    fn identity(&self) -> &str {
        self.manifest_hash()
    }
    fn validate_layout(&self) -> fono_stt::error::SttResult<()> {
        self.validate_layout().map_err(Into::into)
    }
}
