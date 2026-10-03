//! Versioned JSON-lines contract between Fono and backend-specific STT workers.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 3;
pub const MAX_REQUEST_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESPONSE_FRAME_BYTES: usize = 1024 * 1024;

mod timestamps;
pub use timestamps::{words_from_pieces, TimedPiece, TimedSegment, WindowTranscript};
#[cfg(feature = "inference")]
pub mod inference;

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
    #[serde(default)]
    pub supports_window: bool,
    #[serde(default)]
    pub supports_cancel: bool,
    #[serde(default)]
    pub supports_token_timestamps: bool,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            supports_health: true,
            supports_shutdown: true,
            supports_window: true,
            supports_cancel: true,
            supports_token_timestamps: true,
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
    TranscribeWindow {
        #[serde(flatten)]
        meta: RequestMeta,
        model_path: String,
        language: String,
        context: Option<String>,
        audio_start_sample: u64,
        samples_i16_base64: String,
    },
    /// Sent independently while inference is active. The cancelled target returns
    /// an Error with code `cancelled`; this control frame has no separate reply.
    CancelRequest {
        #[serde(flatten)]
        meta: RequestMeta,
        target_request_id: String,
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
            | Self::TranscribeWindow { meta, .. }
            | Self::CancelRequest { meta, .. }
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
    WindowResult {
        protocol_version: u16,
        request_id: String,
        operation_id: String,
        transcript: WindowTranscript,
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
            | Self::WindowResult { request_id, .. }
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
            | Self::WindowResult {
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
        assert!(capabilities.supports_cancel);
        assert!(capabilities.supports_window);
        assert!(capabilities.supports_token_timestamps);
    }

    #[test]
    fn window_and_cancel_round_trip_without_separate_cancel_response() {
        let request = WorkerRequest::TranscribeWindow {
            meta: RequestMeta::new("window-1", Some("7".into())),
            model_path: "модель.bin".into(),
            language: "ru".into(),
            context: Some("Fono".into()),
            audio_start_sample: 16000,
            samples_i16_base64: "AAA=".into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: WorkerRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            decoded,
            WorkerRequest::TranscribeWindow {
                audio_start_sample: 16000,
                ..
            }
        ));
        let cancel = WorkerRequest::CancelRequest {
            meta: RequestMeta::new("cancel-2", Some("7".into())),
            target_request_id: "window-1".into(),
        };
        let json = serde_json::to_string(&cancel).unwrap();
        assert_eq!(
            serde_json::from_str::<WorkerRequest>(&json)
                .unwrap()
                .meta()
                .request_id,
            "cancel-2"
        );
    }
}
