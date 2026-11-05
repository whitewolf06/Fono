//! A recognizer result shared by desktop and server.

use serde::{Deserialize, Serialize};

/// Physical inference backend reported by a remote recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBackend {
    Cpu,
    Cuda,
    Vulkan,
}

impl RemoteBackend {
    pub fn valid_device(self, device: &str) -> bool {
        match self {
            Self::Cpu => device == "cpu",
            Self::Vulkan => device == "vulkan",
            Self::Cuda => {
                device == "cuda"
                    || device.strip_prefix("cuda:").is_some_and(|index| {
                        !index.is_empty()
                            && index.len() <= 3
                            && index.bytes().all(|byte| byte.is_ascii_digit())
                    })
            }
        }
    }
}

/// Facts belonging to one successful request. Never contains an endpoint,
/// credential, server label, hardware name or any source text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTranscriptMetadata {
    pub model: crate::ModelId,
    pub backend: RemoteBackend,
    pub device: String,
    /// Client-observed request duration, not a network-only measurement.
    pub request_ms: u64,
    /// Server-reported total, which can include queueing and loading.
    pub server_total_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_ms: Option<u64>,
    /// Unknown until a transport provides a separate, actual measurement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_ms: Option<u64>,
}

impl RemoteTranscriptMetadata {
    pub fn valid(&self) -> bool {
        const MAX_DURATION_MS: u64 = 86_400_000;
        self.backend.valid_device(&self.device)
            && !self.model.variant.is_empty()
            && self.model.variant.len() <= 128
            && self
                .model
                .variant
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            && self.request_ms <= MAX_DURATION_MS
            && self.server_total_ms <= MAX_DURATION_MS
            && [self.queue_ms, self.load_ms, self.decode_ms]
                .into_iter()
                .flatten()
                .all(|duration| duration <= self.server_total_ms)
            && match self.network_ms {
                Some(duration) => duration <= self.request_ms,
                None => true,
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub detected_language: Option<String>,
    /// Время транскрипции в секундах (для отображения в UI).
    #[serde(default)]
    pub transcribe_secs: Option<f32>,
    /// Длительность аудио в секундах.
    #[serde(default)]
    pub audio_secs: Option<f32>,
    /// Какое устройство использовалось (CPU/CUDA).
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteTranscriptMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelId, WhisperModelSize};

    fn remote() -> RemoteTranscriptMetadata {
        RemoteTranscriptMetadata {
            model: ModelId::whisper(WhisperModelSize::Tiny),
            backend: RemoteBackend::Cuda,
            device: "cuda:0".into(),
            request_ms: 30,
            server_total_ms: 25,
            queue_ms: None,
            load_ms: None,
            decode_ms: None,
            network_ms: None,
        }
    }

    #[test]
    fn legacy_transcripts_keep_unknown_remote_facts_absent() {
        let legacy =
            serde_json::json!({"text":"fixture", "detected_language":"ru", "device":"CPU"});
        let transcript: Transcript = serde_json::from_value(legacy).unwrap();
        assert!(transcript.remote.is_none());
        assert!(transcript.transcribe_secs.is_none());
        assert!(serde_json::to_value(transcript)
            .unwrap()
            .get("remote")
            .is_none());
        let metadata = remote();
        let encoded = serde_json::to_value(&metadata).unwrap();
        assert!(encoded.get("decode_ms").is_none());
        assert!(encoded.get("network_ms").is_none());
        assert_eq!(
            serde_json::from_value::<RemoteTranscriptMetadata>(encoded).unwrap(),
            metadata
        );
    }

    #[test]
    fn remote_facts_reject_unbounded_or_inconsistent_values() {
        assert!(remote().valid());
        for change in [
            |value: &mut RemoteTranscriptMetadata| value.device = "PRIVATE_GPU_NAME".into(),
            |value: &mut RemoteTranscriptMetadata| value.backend = RemoteBackend::Cpu,
            |value: &mut RemoteTranscriptMetadata| value.model.variant = "https://PRIVATE".into(),
            |value: &mut RemoteTranscriptMetadata| value.request_ms = 86_400_001,
            |value: &mut RemoteTranscriptMetadata| value.server_total_ms = 86_400_001,
            |value: &mut RemoteTranscriptMetadata| value.decode_ms = Some(26),
            |value: &mut RemoteTranscriptMetadata| value.queue_ms = Some(26),
            |value: &mut RemoteTranscriptMetadata| value.load_ms = Some(26),
            |value: &mut RemoteTranscriptMetadata| value.network_ms = Some(31),
        ] {
            let mut value = remote();
            change(&mut value);
            assert!(!value.valid());
        }
        // Independent measured clocks are retained, never subtracted to invent network time.
        let mut value = remote();
        value.request_ms = 1;
        assert!(value.valid());
    }
}
