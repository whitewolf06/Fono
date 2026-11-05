//! Private API DTOs. Unknown fields allow additive API v2 updates; required
//! fields, bounded bodies and semantic validation reject malformed replies.
use super::{failure, RemoteErrorKind};
use crate::error::AppResult;
use fono_types::{RemoteBackend, SttEngine, SubtitleSegment};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Capabilities {
    pub product: String,
    pub api_version: u16,
    pub audio_formats: Vec<String>,
    pub max_audio_seconds: u64,
    pub max_upload_bytes: u64,
    pub features: Features,
}
#[derive(Deserialize)]
pub(super) struct Features {
    pub cancel_on_disconnect: bool,
    pub glossary: bool,
    pub timestamps: Vec<String>,
    pub decoding: Vec<String>,
    #[serde(default)]
    pub jobs: bool,
}
#[derive(Deserialize)]
pub(super) struct Models {
    pub object: String,
    pub data: Vec<Model>,
}
#[derive(Deserialize)]
pub(super) struct Model {
    pub id: String,
    pub object: String,
    pub fono: Details,
}
#[derive(Deserialize)]
pub(super) struct Details {
    pub engine: SttEngine,
    pub languages: Vec<String>,
    pub state: String,
    pub disk_state: String,
    pub device: String,
    pub features: ModelFeatures,
}
#[derive(Deserialize)]
pub(super) struct ModelFeatures {
    pub glossary: bool,
    pub timestamps: Vec<String>,
    pub decoding: Vec<String>,
}
#[derive(Deserialize)]
pub(super) struct Recognition {
    pub text: String,
    pub language: Option<String>,
    pub model: String,
    pub backend: Option<String>,
    pub device: Option<String>,
    pub segments: Vec<SubtitleSegment>,
    pub timings: Timings,
}
#[derive(Deserialize)]
pub(super) struct Timings {
    pub audio_ms: u64,
    pub total_ms: u64,
    #[serde(default)]
    pub queue_ms: Option<u64>,
    #[serde(default)]
    pub load_ms: Option<u64>,
    #[serde(default)]
    pub decode_ms: Option<u64>,
}

impl Recognition {
    pub(super) fn validate(&self, model: &str, samples: usize) -> AppResult<()> {
        let duration = (samples as u64).div_ceil(16);
        if self.model != model
            || self.timings.audio_ms.abs_diff(duration) > 1
            || self.timings.total_ms > 86_400_000
            || [
                self.timings.queue_ms,
                self.timings.load_ms,
                self.timings.decode_ms,
            ]
            .into_iter()
            .flatten()
            .any(|duration| duration > self.timings.total_ms)
            || self.language.as_ref().is_some_and(|l| {
                l.len() > 16 || !l.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            })
            || !matches!(self.backend.as_deref(), Some("cpu" | "cuda" | "vulkan"))
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        device(self.device.as_deref().unwrap_or_default())?;
        let backend = match self.backend.as_deref() {
            Some("cpu") => RemoteBackend::Cpu,
            Some("cuda") => RemoteBackend::Cuda,
            Some("vulkan") => RemoteBackend::Vulkan,
            _ => return failure(RemoteErrorKind::InvalidResponse),
        };
        if !backend.valid_device(self.device.as_deref().unwrap_or_default()) {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        let mut previous_start = 0;
        for segment in &self.segments {
            if segment.start_ms < previous_start
                || segment.start_ms > segment.end_ms
                || segment.end_ms > duration
                || segment.start_ms * 16 > samples as u64
            {
                return failure(RemoteErrorKind::InvalidResponse);
            }
            previous_start = segment.start_ms;
        }
        Ok(())
    }
}

pub(super) fn device(device: &str) -> AppResult<String> {
    if device == "cpu"
        || device == "vulkan"
        || device == "cuda"
        || device
            .strip_prefix("cuda:")
            .is_some_and(|v| !v.is_empty() && v.len() <= 3 && v.bytes().all(|b| b.is_ascii_digit()))
    {
        Ok(format!("remote:{device}"))
    } else {
        failure(RemoteErrorKind::InvalidResponse)
    }
}
