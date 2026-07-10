//! Stable JSON-lines contract between Fono UI and backend-specific STT workers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Cuda,
    Vulkan,
    Cpu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Ping,
    Transcribe {
        id: String,
        model_path: String,
        language: String,
        /// PCM i16 little-endian encoded as base64.
        samples_i16_base64: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResponse {
    Ready { backend: BackendKind },
    Result {
        id: String,
        text: String,
        audio_secs: f32,
        transcribe_secs: f32,
        backend: BackendKind,
    },
    Error {
        id: Option<String>,
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_as_json_line() {
        let request = WorkerRequest::Transcribe {
            id: "request-1".into(),
            model_path: "model.bin".into(),
            language: "auto".into(),
            samples_i16_base64: "AAE=".into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(matches!(
            serde_json::from_str::<WorkerRequest>(&json).unwrap(),
            WorkerRequest::Transcribe { id, .. } if id == "request-1"
        ));
    }
}
