//! Versioned JSON-lines contract between Fono and backend-specific STT workers.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_REQUEST_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESPONSE_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Cuda,
    Vulkan,
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestMeta {
    pub protocol_version: u16,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

impl RequestMeta {
    pub fn new(request_id: impl Into<String>, operation_id: Option<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            operation_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    pub protocol_version: u16,
    pub supports_health: bool,
    pub supports_shutdown: bool,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            supports_health: true,
            supports_shutdown: true,
            maximum_request_bytes: MAX_REQUEST_FRAME_BYTES,
            maximum_response_bytes: MAX_RESPONSE_FRAME_BYTES,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello {
        #[serde(flatten)]
        meta: RequestMeta,
    },
    Ping {
        #[serde(flatten)]
        meta: RequestMeta,
    },
    Load {
        #[serde(flatten)]
        meta: RequestMeta,
        model_path: String,
    },
    Transcribe {
        #[serde(flatten)]
        meta: RequestMeta,
        model_path: String,
        language: String,
        /// PCM i16 little-endian encoded as base64.
        samples_i16_base64: String,
    },
    Shutdown {
        #[serde(flatten)]
        meta: RequestMeta,
    },
}

impl WorkerRequest {
    pub fn meta(&self) -> &RequestMeta {
        match self {
            Self::Hello { meta }
            | Self::Ping { meta }
            | Self::Load { meta, .. }
            | Self::Transcribe { meta, .. }
            | Self::Shutdown { meta } => meta,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResponse {
    Ready {
        protocol_version: u16,
        request_id: String,
        backend: BackendKind,
        capabilities: WorkerCapabilities,
    },
    Pong {
        protocol_version: u16,
        request_id: String,
        backend: BackendKind,
    },
    ModelLoaded {
        protocol_version: u16,
        request_id: String,
        backend: BackendKind,
    },
    Result {
        protocol_version: u16,
        request_id: String,
        operation_id: String,
        text: String,
        audio_secs: f32,
        transcribe_secs: f32,
        backend: BackendKind,
    },
    ShuttingDown {
        protocol_version: u16,
        request_id: String,
    },
    Error {
        protocol_version: u16,
        request_id: String,
        operation_id: Option<String>,
        code: String,
        message: String,
    },
}

impl WorkerResponse {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Ready { request_id, .. }
            | Self::Pong { request_id, .. }
            | Self::ModelLoaded { request_id, .. }
            | Self::Result { request_id, .. }
            | Self::ShuttingDown { request_id, .. }
            | Self::Error { request_id, .. } => request_id,
        }
    }

    pub fn protocol_version(&self) -> u16 {
        match self {
            Self::Ready {
                protocol_version, ..
            }
            | Self::Pong {
                protocol_version, ..
            }
            | Self::ModelLoaded {
                protocol_version, ..
            }
            | Self::Result {
                protocol_version, ..
            }
            | Self::ShuttingDown {
                protocol_version, ..
            }
            | Self::Error {
                protocol_version, ..
            } => *protocol_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_with_version_and_ids() {
        let request = WorkerRequest::Transcribe {
            meta: RequestMeta::new("request-1", Some("operation-7".into())),
            model_path: "model.bin".into(),
            language: "auto".into(),
            samples_i16_base64: "AAE=".into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded = serde_json::from_str::<WorkerRequest>(&json).unwrap();
        assert_eq!(decoded.meta().protocol_version, PROTOCOL_VERSION);
        assert_eq!(decoded.meta().request_id, "request-1");
        assert_eq!(decoded.meta().operation_id.as_deref(), Some("operation-7"));
    }

    #[test]
    fn load_request_has_a_versioned_envelope() {
        let request = WorkerRequest::Load {
            meta: RequestMeta::new("load-1", None),
            model_path: "model.bin".into(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["type"], "load");
        assert_eq!(json["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(json["request_id"], "load-1");
        assert!(json.get("operation_id").is_none());
    }

    #[test]
    fn capabilities_publish_transport_limits() {
        let capabilities = WorkerCapabilities::default();
        assert_eq!(capabilities.protocol_version, PROTOCOL_VERSION);
        assert_eq!(capabilities.maximum_request_bytes, MAX_REQUEST_FRAME_BYTES);
        assert_eq!(
            capabilities.maximum_response_bytes,
            MAX_RESPONSE_FRAME_BYTES
        );
        assert!(capabilities.supports_health);
        assert!(capabilities.supports_shutdown);
    }
}
