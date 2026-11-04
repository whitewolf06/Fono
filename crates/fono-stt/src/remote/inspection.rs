//! Bounded, metadata-only connection inspection; no audio or model activation.
use super::{
    failure, transport::Transport, wire, RemoteEndpoint, RemoteError, RemoteErrorKind,
    RemoteServerHealth, PCM_TYPE,
};
use crate::{error::AppResult, ActivityGuard};
use fono_core::OperationCancellation;
use fono_types::{ModelId, SttEngine};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteInspectionState {
    Ready,
    Starting,
    Busy,
    Failed,
    Stopping,
}
#[derive(Debug, Clone, Serialize)]
pub struct RemoteDevice {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub available: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct RemoteModel {
    pub id: ModelId,
    pub label: String,
    pub installed: bool,
    pub state: String,
    pub languages: Vec<String>,
    pub device: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct RemoteInspection {
    pub version: String,
    pub api_version: u16,
    pub state: RemoteInspectionState,
    pub devices: Vec<RemoteDevice>,
    pub models: Vec<RemoteModel>,
}

#[derive(Deserialize)]
struct Description {
    #[serde(flatten)]
    compatibility: wire::Capabilities,
    version: String,
    devices: Vec<Device>,
}
#[derive(Deserialize)]
struct Device {
    id: String,
    kind: String,
    name: Option<String>,
}
#[derive(Deserialize)]
struct Health {
    status: RemoteServerHealth,
    queue: Queue,
}
#[derive(Deserialize)]
struct Queue {
    interactive: u32,
    batch: u32,
}

/// Blocking host entry point. The same TLS/auth policy and single discovery
/// deadline cover every metadata request. No recognizer is created or loaded.
pub fn inspect_server(
    endpoint: RemoteEndpoint,
    selected: Option<&ModelId>,
    activity: Arc<dyn ActivityGuard>,
    cancellation: &OperationCancellation,
) -> AppResult<RemoteInspection> {
    let selected = selected.cloned();
    let http = Arc::new(Transport::new(endpoint, activity)?);
    let task = http.clone();
    http.call(cancellation, http.discovery_deadline(), || {
        Ok(async move {
            let description = task
                .json::<Description>(task.get("v1/capabilities"), 64 * 1024)
                .await
                .map_err(|error| match error {
                    crate::error::SttError::Remote(RemoteError {
                        status: Some(404), ..
                    }) => RemoteError::new(RemoteErrorKind::Incompatible).into(),
                    other => other,
                })?;
            validate_description(&description)?;
            let models = task
                .json::<wire::Models>(task.get("v1/models"), 512 * 1024)
                .await?;
            let (status, health) = task
                .health_json::<Health>(task.get("v1/health"), 64 * 1024)
                .await?;
            if (status == 200) != (health.status == RemoteServerHealth::Ready) {
                return failure(RemoteErrorKind::InvalidResponse);
            }
            let models = model_inventory(models)?;
            if let Some(selected) = selected {
                let Some(model) = models
                    .iter()
                    .find(|model| model.id == selected && model.installed)
                else {
                    return failure(RemoteErrorKind::ModelNotFound);
                };
                if model.state == "failed" {
                    return failure(RemoteErrorKind::WorkerFailed);
                }
            }
            let state = match health.status {
                RemoteServerHealth::Ready
                    if health.queue.interactive > 0
                        || health.queue.batch > 0
                        || models.iter().any(|model| model.state == "busy") =>
                {
                    RemoteInspectionState::Busy
                }
                RemoteServerHealth::Ready => RemoteInspectionState::Ready,
                RemoteServerHealth::Starting => RemoteInspectionState::Starting,
                RemoteServerHealth::Failed => RemoteInspectionState::Failed,
                RemoteServerHealth::Stopping => RemoteInspectionState::Stopping,
            };
            let mut identities = HashSet::new();
            let mut devices = Vec::new();
            for device in description.devices {
                wire::device(&device.id)?;
                if !matches!(device.kind.as_str(), "cpu" | "cuda" | "vulkan")
                    || !identities.insert(device.id.clone())
                    || !device.id.starts_with(&device.kind)
                {
                    return failure(RemoteErrorKind::InvalidResponse);
                }
                let name = device.name.unwrap_or_else(|| device.kind.to_uppercase());
                if !display_text(&name, 160) {
                    return failure(RemoteErrorKind::InvalidResponse);
                }
                devices.push(RemoteDevice {
                    id: device.id,
                    name,
                    backend: device.kind,
                    available: true,
                });
            }
            Ok(RemoteInspection {
                version: description.version,
                api_version: description.compatibility.api_version,
                state,
                devices,
                models,
            })
        })
    })
}

fn validate_description(description: &Description) -> AppResult<()> {
    let server = &description.compatibility;
    if server.product != "fono-server"
        || server.api_version < 2
        || !server.audio_formats.iter().any(|format| format == PCM_TYPE)
        || !server.features.cancel_on_disconnect
        || server.max_audio_seconds == 0
        || server.max_upload_bytes < 2
        || !server.features.decoding.iter().any(|mode| mode == "fast")
    {
        return failure(RemoteErrorKind::Incompatible);
    }
    if description.version.is_empty()
        || description.version.len() > 64
        || !description
            .version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_'))
        || description.devices.is_empty()
        || description.devices.len() > 64
    {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    Ok(())
}

fn model_inventory(models: wire::Models) -> AppResult<Vec<RemoteModel>> {
    if models.object != "list" || models.data.len() > 256 {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    let mut identities = HashSet::new();
    let mut inventory = Vec::new();
    for model in models.data {
        let engine = match model.fono.engine {
            SttEngine::Whisper => "whisper",
            SttEngine::GigaAm => "giga_am",
            SttEngine::Parakeet => "parakeet",
        };
        let variant = model
            .id
            .strip_prefix(&format!("{engine}:"))
            .filter(|variant| {
                !variant.is_empty()
                    && variant.len() <= 128
                    && variant.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
            })
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?;
        if model.object != "model"
            || !identities.insert(model.id.clone())
            || !matches!(
                model.fono.state.as_str(),
                "ready" | "busy" | "loading" | "retiring" | "unloaded" | "failed"
            )
            || !matches!(
                model.fono.disk_state.as_str(),
                "verified" | "present_unverified" | "missing" | "invalid"
            )
            || model.fono.languages.len() > 64
            || model.fono.languages.iter().any(|language| {
                language.is_empty()
                    || language.len() > 16
                    || !language
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            })
            || !model
                .fono
                .features
                .decoding
                .iter()
                .any(|mode| mode == "fast")
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        wire::device(&model.fono.device)?;
        inventory.push(RemoteModel {
            id: ModelId {
                engine: model.fono.engine,
                variant: variant.to_owned(),
            },
            label: model.id,
            installed: matches!(
                model.fono.disk_state.as_str(),
                "verified" | "present_unverified"
            ),
            state: model.fono.state,
            languages: model.fono.languages,
            device: model.fono.device,
        });
    }
    Ok(inventory)
}

fn display_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
