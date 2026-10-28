//! Stable speech-model identity, independent of a backend's local file path.
use crate::{CoreError, CoreResult, ErrorCode, WhisperArtifact, WhisperModelSize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttEngine {
    Whisper,
    GigaAm,
    Parakeet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelId {
    pub engine: SttEngine,
    pub variant: String,
}

impl ModelId {
    pub fn whisper_artifact(&self) -> CoreResult<WhisperArtifact> {
        if self.engine == SttEngine::Whisper {
            if let Some(model) = WhisperArtifact::all()?
                .into_iter()
                .find(|m| m.variant == self.variant)
            {
                return Ok(model);
            }
        }
        Err(invalid_request("Unknown managed Whisper model".into()))
    }

    pub fn public_id(&self) -> String {
        match self.engine {
            SttEngine::Whisper => self.variant.clone(),
            SttEngine::GigaAm => format!("giga_am:{}", self.variant),
            SttEngine::Parakeet => format!("parakeet:{}", self.variant),
        }
    }

    pub fn whisper(size: WhisperModelSize) -> Self {
        Self {
            engine: SttEngine::Whisper,
            variant: size.api_identifier().into(),
        }
    }

    /// Other backends acquire runnable artifacts in F-01.3; never route them
    /// through the Whisper loader merely because their files exist.
    pub fn whisper_size(&self) -> CoreResult<WhisperModelSize> {
        if self.engine != SttEngine::Whisper {
            return Err(invalid_request("STT model is not a Whisper model".into()));
        }
        WhisperModelSize::ALL
            .into_iter()
            .find(|size| size.api_identifier() == self.variant)
            .ok_or_else(|| invalid_request("Unknown Whisper model".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SttModelSelection {
    pub id: ModelId,
    pub local_path: String,
}

impl SttModelSelection {
    pub fn from_whisper_path(path: String) -> Self {
        // Settings may move between platforms. Preserve the exact original
        // path, including custom models; only inspect its final component.
        let filename = path.rsplit(['/', '\\']).next().unwrap_or(&path);
        let id = WhisperArtifact::from_filename(filename)
            .map(|model| model.id())
            .unwrap_or(ModelId {
                engine: SttEngine::Whisper,
                variant: "custom".into(),
            });
        Self {
            id,
            local_path: path,
        }
    }

    /// Validate the path and managed Whisper identity. Runtimes must additionally
    /// validate non-Whisper identities against their enabled model registry.
    pub fn validate(&self) -> CoreResult<()> {
        if self.local_path.trim().is_empty() {
            return Err(invalid_request("STT model path is empty".into()));
        }
        // Backend availability and offline model registries belong to the runtime.
        if self.id.engine != SttEngine::Whisper {
            return Ok(());
        }
        if self.id.variant == "custom" {
            return Ok(());
        }
        let artifact = self.id.whisper_artifact()?;
        let filename = self.local_path.rsplit(['/', '\\']).next();
        if filename != Some(artifact.filename.as_str()) {
            return Err(invalid_request(
                "STT model identity does not match its file".into(),
            ));
        }
        Ok(())
    }
}

fn invalid_request(message: String) -> CoreError {
    CoreError::new(ErrorCode::InvalidRequest, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_custom_paths_keep_cross_platform_identity_and_validation() {
        for (path, variant) in [
            (r"D:\models\ggml-small.bin", "small"),
            ("/models/ggml-large-v3-turbo-q8_0.bin", "large_turbo_q8_0"),
            ("/models/my-model.bin", "custom"),
        ] {
            let selection = SttModelSelection::from_whisper_path(path.into());
            assert_eq!(selection.local_path, path);
            assert_eq!(selection.id.variant, variant);
            selection.validate().unwrap();
        }
        let mismatched = SttModelSelection {
            id: ModelId::whisper(WhisperModelSize::Tiny),
            local_path: "/models/ggml-base.bin".into(),
        };
        assert_eq!(
            mismatched.validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(SttModelSelection::from_whisper_path("  ".into())
            .validate()
            .is_err());
    }

    #[test]
    fn invalid_model_errors_do_not_echo_untrusted_identifiers() {
        let id = ModelId {
            engine: SttEngine::Whisper,
            variant: "PRIVATE /home/profile/model.bin".into(),
        };
        for error in [
            id.whisper_artifact().unwrap_err(),
            id.whisper_size().unwrap_err(),
        ] {
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(!error.retryable);
            assert!(!error.message.contains("PRIVATE"));
            assert!(!error.message.contains("/home/"));
        }
    }
}
