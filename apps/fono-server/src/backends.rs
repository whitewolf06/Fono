//! Fixed installed workers and device-scoped CUDA selection for the server.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use fono_stt::error::{SttError, SttResult};
use fono_stt::{GpuPackage, GpuPackageHandle, WorkerPaths};
use serde::Serialize;

use crate::config::ServerConfig;

pub fn cuda_enabled() -> bool {
    cfg!(all(
        target_os = "linux",
        feature = "cuda-worker",
        not(feature = "cpu-only")
    ))
}

pub fn cuda_index(device: &str) -> Option<u32> {
    let index = device.strip_prefix("cuda:")?;
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = index.parse::<u32>().ok()?;
    (index == parsed.to_string()).then_some(parsed)
}

pub fn configured_cuda_devices(config: &ServerConfig) -> Vec<u32> {
    let mut indices: Vec<_> = config
        .models
        .placement
        .iter()
        .filter_map(|placement| cuda_index(&placement.device))
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

pub fn reported_device(actual: &str, placement: &str) -> String {
    if actual.eq_ignore_ascii_case("cuda") && cuda_index(placement).is_some() {
        placement.to_owned()
    } else {
        actual.to_ascii_lowercase()
    }
}

pub fn installed_worker() -> Result<GpuPackageHandle, String> {
    if !cuda_enabled() {
        return Err("CUDA worker support is unavailable in this build".into());
    }
    let executable =
        std::env::current_exe().map_err(|_| "Cannot locate the server installation")?;
    let path = executable
        .parent()
        .ok_or("Cannot locate the server installation")?
        .join("fono-stt-cuda-worker");
    package_at(&path)
}

struct InstalledPackage {
    path: PathBuf,
    file: File,
    identity: String,
    modified: Option<SystemTime>,
}

fn package_at(path: &Path) -> Result<GpuPackageHandle, String> {
    validate_worker_path(path).map_err(|_| "CUDA worker installation is missing or unsafe")?;
    let file = File::open(path).map_err(|_| "Cannot open the installed CUDA worker")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the installed CUDA worker")?;
    Ok(GpuPackageHandle::new(InstalledPackage {
        path: path.to_owned(),
        modified: metadata.modified().ok(),
        identity: format!("installed-cuda:{}", path.display()),
        file,
    }))
}

fn validate_worker_path(path: &Path) -> SttResult<()> {
    if !path.is_absolute() {
        return Err(SttError::Config("Worker path must be absolute".into()));
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|_| SttError::Config("Worker installation is unavailable".into()))?;
        if metadata.file_type().is_symlink()
            || (ancestor == path && !metadata.is_file())
            || (ancestor != path && !metadata.is_dir())
        {
            return Err(SttError::Config(
                "Worker installation must not contain symbolic links".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mode = metadata.mode();
            let trusted_sticky_parent =
                ancestor != path && mode & 0o1000 != 0 && metadata.uid() == 0;
            if (metadata.uid() != 0 && metadata.uid() != unsafe { libc::geteuid() })
                || (mode & 0o022 != 0 && !trusted_sticky_parent)
                || (ancestor == path && mode & 0o111 == 0)
            {
                return Err(SttError::Config(
                    "Worker installation has unsafe ownership or permissions".into(),
                ));
            }
        }
    }
    Ok(())
}

impl GpuPackage for InstalledPackage {
    fn worker_path(&self) -> &Path {
        &self.path
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn validate_layout(&self) -> SttResult<()> {
        validate_worker_path(&self.path)?;
        let held = self
            .file
            .metadata()
            .map_err(|_| SttError::Config("Worker installation changed".into()))?;
        let current = fs::metadata(&self.path)
            .map_err(|_| SttError::Config("Worker installation changed".into()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if held.dev() != current.dev() || held.ino() != current.ino() {
                return Err(SttError::Config(
                    "Worker installation changed; restart the server".into(),
                ));
            }
        }
        if held.len() != current.len() || current.modified().ok() != self.modified {
            return Err(SttError::Config(
                "Worker installation changed; restart the server".into(),
            ));
        }
        Ok(())
    }
}

pub fn worker_paths(config: &ServerConfig) -> Result<WorkerPaths, String> {
    let needs_cuda = !configured_cuda_devices(config).is_empty();
    let mut paths = WorkerPaths::default();
    if needs_cuda || (cuda_enabled() && installed_worker().is_ok()) {
        paths = paths.with_cuda_package(installed_worker()?);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let vad = parent.join("silero_vad.onnx");
            if vad.is_file() {
                paths = paths.with_speech_vad(vad);
            }
        }
    }
    Ok(paths)
}

#[derive(Debug, Clone, Serialize)]
pub struct CudaDevice {
    pub index: u32,
    pub name: String,
    pub memory_total_mb: u64,
    pub memory_free_mb: u64,
    pub driver_version: String,
}

fn parse_devices(output: &str) -> Result<Vec<CudaDevice>, String> {
    let mut devices = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 5 {
            return Err("Cannot parse NVIDIA device inventory".into());
        }
        let index = fields[0]
            .parse()
            .map_err(|_| "Invalid NVIDIA device index")?;
        let memory_total_mb = fields[2]
            .parse()
            .map_err(|_| "Invalid NVIDIA memory size")?;
        let memory_free_mb = fields[3]
            .parse()
            .map_err(|_| "Invalid NVIDIA free memory")?;
        if memory_total_mb == 0
            || memory_free_mb > memory_total_mb
            || fields[1].is_empty()
            || fields[1].len() > 256
            || fields[4].is_empty()
            || !fields[4]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            || devices
                .iter()
                .any(|device: &CudaDevice| device.index == index)
        {
            return Err("Invalid NVIDIA device inventory".into());
        }
        devices.push(CudaDevice {
            index,
            name: fields[1].to_owned(),
            memory_total_mb,
            memory_free_mb,
            driver_version: fields[4].to_owned(),
        });
    }
    Ok(devices)
}

/// Bounded driver inventory. No driver installation or device state is changed.
pub fn cuda_devices() -> Result<Vec<CudaDevice>, String> {
    if !cuda_enabled() {
        return Ok(Vec::new());
    }
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|_| "NVIDIA driver inventory is unavailable")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("NVIDIA driver inventory failed".into()),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("NVIDIA driver inventory failed".into());
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("NVIDIA driver inventory timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()
        .ok_or("NVIDIA inventory output is unavailable")?
        .take(65_537)
        .read_to_string(&mut output)
        .map_err(|_| "Cannot read NVIDIA device inventory")?;
    if output.len() > 65_536 {
        return Err("NVIDIA device inventory exceeds its limit".into());
    }
    parse_devices(&output)
}

pub fn validate_devices(config: &ServerConfig) -> Result<(), String> {
    let requested = configured_cuda_devices(config);
    if requested.is_empty() {
        return Ok(());
    }
    installed_worker()?;
    let available = cuda_devices()?;
    if requested
        .iter()
        .any(|index| !available.iter().any(|device| device.index == *index))
    {
        return Err("A configured CUDA device is unavailable".into());
    }
    Ok(())
}

pub fn probe_worker(activity: Arc<dyn fono_stt::ActivityGuard>) -> Result<(), String> {
    fono_stt::probe_cuda_worker(installed_worker()?, activity)
        .map_err(|_| "CUDA worker handshake failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpu_inventory_preserves_observed_values_and_rejects_invalid_metrics() {
        let devices = parse_devices("0, NVIDIA GeForce RTX 3060 Ti, 8192, 6944, 595.84\n").unwrap();
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].memory_free_mb, 6944);
        for invalid in [
            "0, GPU, 0, 0, 595.84",
            "0, GPU, 8192, 9000, 595.84",
            "0, GPU, N/A, 0, 595.84",
            "0, GPU, 8192, 10, unknown",
        ] {
            assert!(parse_devices(invalid).is_err());
        }
    }
    #[test]
    fn cuda_ordinals_are_canonical_and_bounded() {
        assert_eq!(cuda_index("cuda:0"), Some(0));
        assert_eq!(cuda_index("cuda:12"), Some(12));
        for invalid in ["cpu", "cuda:-1", "cuda:01", "cuda:4294967296", "cuda:"] {
            assert_eq!(cuda_index(invalid), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn installed_package_rejects_links_writable_paths_and_replacement() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory =
            std::env::temp_dir().join(format!("fono-worker-layout-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("fono-stt-cuda-worker");
        fs::write(&path, b"worker").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let held = package_at(&path).unwrap();
        held.validate_layout().unwrap();
        let link = directory.join("linked-worker");
        symlink(&path, &link).unwrap();
        assert!(package_at(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(held.validate_layout().is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let replacement = directory.join("replacement");
        fs::write(&replacement, b"worker").unwrap();
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o755)).unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert!(held.validate_layout().is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
