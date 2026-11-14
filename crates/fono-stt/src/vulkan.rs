//! Bounded physical-device discovery and persistent shader-cache preparation.
//! The worker performs the same physical-device check again before inference.
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{SttError, SttResult};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VulkanDevice {
    pub index: u32,
    pub name: String,
    pub uuid: String,
    pub driver: String,
    pub discrete: bool,
    pub supported: bool,
    pub memory_total_bytes: u64,
    pub memory_free_bytes: Option<u64>,
    /// Per-process driver budget; never publish this as global free VRAM.
    #[serde(default)]
    pub memory_budget_free_bytes: Option<u64>,
}

impl VulkanDevice {
    pub fn suitable_discrete_gpu(&self) -> bool {
        self.discrete && self.supported && self.memory_total_bytes > 0
    }
}

#[derive(Deserialize)]
struct Inventory {
    schema_version: u32,
    devices: Vec<VulkanDevice>,
}

fn parse(bytes: &[u8]) -> SttResult<Vec<VulkanDevice>> {
    let inventory: Inventory = serde_json::from_slice(bytes)?;
    let mut indices = HashSet::new();
    if inventory.schema_version != 1 || inventory.devices.len() > 64 {
        return Err(SttError::Config(
            "Unsupported Vulkan device inventory".into(),
        ));
    }
    for device in &inventory.devices {
        if !indices.insert(device.index)
            || device.index >= 64
            || device.name.is_empty()
            || device.name.len() > 256
            || device.driver.len() > 256
            || device.uuid.len() != 32
            || !device.uuid.bytes().all(|byte| byte.is_ascii_hexdigit())
            || device
                .memory_free_bytes
                .is_some_and(|free| free > device.memory_total_bytes)
        {
            return Err(SttError::Config("Invalid Vulkan device inventory".into()));
        }
    }
    Ok(inventory.devices)
}

/// Caller must run this on a blocking thread. No model or GPU context is opened.
pub fn devices(worker: &Path) -> SttResult<Vec<VulkanDevice>> {
    let mut command = Command::new(worker);
    command
        .arg("--devices-json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("piped inventory output");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(65_537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| SttError::Stt("Vulkan inventory reader failed".into()))??;
    // A CPU-only Windows host may have no Vulkan loader installed. The
    // executable exists, but Windows cannot resolve its optional GPU imports.
    #[cfg(windows)]
    if status.is_some_and(|status| status.code() == Some(0xc0000135_u32 as i32)) {
        return Err(SttError::BackendUnavailable(
            crate::error::RecognitionBackend::Vulkan,
        ));
    }
    if !status.is_some_and(|status| status.success()) || bytes.len() > 65_536 {
        return Err(SttError::Stt(
            "Vulkan driver inventory failed or timed out".into(),
        ));
    }
    parse(&bytes)
}

pub fn selected_device(worker: &Path) -> SttResult<Option<VulkanDevice>> {
    let devices = devices(worker)?;
    let selector = std::env::var_os("GGML_VK_VISIBLE_DEVICES");
    select_device(devices, selector.as_deref())
}

fn select_device(
    devices: Vec<VulkanDevice>,
    selector: Option<&std::ffi::OsStr>,
) -> SttResult<Option<VulkanDevice>> {
    if let Some(selector) = selector {
        let device = devices
            .into_iter()
            .find(|device| selector == std::ffi::OsStr::new(&device.index.to_string()))
            .ok_or_else(|| {
                SttError::Config("Select one canonical physical Vulkan device index".into())
            })?;
        return if device.suitable_discrete_gpu() {
            Ok(Some(device))
        } else {
            Err(SttError::Config(
                "Selected Vulkan device is not a supported discrete GPU".into(),
            ))
        };
    }
    Ok(devices
        .into_iter()
        .find(VulkanDevice::suitable_discrete_gpu))
}

pub fn cache_directory() -> Option<PathBuf> {
    let absolute = |key: &str| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if cfg!(debug_assertions) {
        if let Some(root) = absolute("FONO_TEST_DATA_DIR") {
            return Some(root.join("cache/vulkan"));
        }
    }
    absolute("XDG_CACHE_HOME")
        .map(|root| root.join("fono/vulkan"))
        .or_else(|| absolute("LOCALAPPDATA").map(|root| root.join("Fono/cache/vulkan")))
        .or_else(|| absolute("HOME").map(|root| root.join(".cache/fono/vulkan")))
}

pub(crate) fn configure_cache(command: &mut Command) {
    let Some(root) = cache_directory() else {
        return;
    };
    let graphics = root.join("graphics");
    let mesa = root.join("mesa");
    if std::fs::create_dir_all(&graphics)
        .and_then(|()| std::fs::create_dir_all(&mesa))
        .is_err()
    {
        tracing::warn!(
            "Persistent Vulkan shader cache is unavailable; model warm-up is still required"
        );
        return;
    }
    command
        .env("__GL_SHADER_DISK_CACHE", "1")
        .env("__GL_SHADER_DISK_CACHE_PATH", graphics)
        .env("MESA_SHADER_CACHE_DIR", mesa);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device(discrete: bool, supported: bool) -> serde_json::Value {
        serde_json::json!({"index":0,"name":"fixture","uuid":"00112233445566778899aabbccddeeff",
            "driver":"fixture","discrete":discrete,"supported":supported,
            "memory_total_bytes":8192,"memory_free_bytes":null})
    }
    #[test]
    fn software_integrated_and_unsupported_devices_cannot_enable_auto_gpu() {
        for (discrete, supported) in [(false, true), (false, false), (true, false), (true, true)] {
            let bytes = serde_json::to_vec(
                &serde_json::json!({"schema_version":1,"devices":[device(discrete,supported)]}),
            )
            .unwrap();
            let devices = parse(&bytes).unwrap();
            assert_eq!(devices[0].suitable_discrete_gpu(), discrete && supported);
            assert_eq!(devices[0].memory_free_bytes, None);
        }
    }
    #[test]
    fn malformed_metrics_schema_and_duplicate_indices_are_rejected() {
        let mut invalid = device(true, true);
        invalid["memory_free_bytes"] = serde_json::json!(9000);
        for value in [
            serde_json::json!({"schema_version":1,"devices":[invalid]}),
            serde_json::json!({"schema_version":2,"devices":[]}),
            serde_json::json!({"schema_version":1,"devices":[device(true,true),device(true,true)]}),
        ] {
            assert!(parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
}
