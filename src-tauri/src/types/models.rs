//! Audio devices, local Whisper models and acceleration selection.

pub use fono_types::{
    AccelerationCapabilities, AccelerationMode, DecodingOptions, DecodingStrategy, WhisperModelSize,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperModelInfo {
    pub filename: String,
    pub size: WhisperModelSize,
    /// `Some(path)` если модель уже скачана локально.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// GPU residency is independent of the selected inference backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuModelResidency {
    #[default]
    Resident,
    Adaptive,
}

pub fn acceleration_capabilities() -> AccelerationCapabilities {
    AccelerationCapabilities {
        cuda: cfg!(feature = "cuda"),
        vulkan: cfg!(feature = "vulkan"),
    }
}
