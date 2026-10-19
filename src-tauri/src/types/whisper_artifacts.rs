//! Managed Whisper artifacts. Quantization is an artifact identity, not a size.
use super::{ModelId, SttEngine, WhisperModelSize};
use crate::error::{AppError, AppResult};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct WhisperArtifact {
    pub variant: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize)]
struct QuantizedManifest {
    schema: u32,
    repository: String,
    revision: String,
    models: Vec<WhisperArtifact>,
}

impl WhisperArtifact {
    pub fn id(&self) -> ModelId {
        ModelId {
            engine: SttEngine::Whisper,
            variant: self.variant.clone(),
        }
    }

    pub fn from_size(size: WhisperModelSize) -> Self {
        Self {
            variant: size.api_identifier().into(),
            filename: size.filename().into(),
            bytes: size.exact_bytes(),
            sha256: size.sha256().into(),
            url: size.url().into(),
        }
    }

    pub fn all() -> AppResult<Vec<Self>> {
        let manifest: QuantizedManifest = serde_json::from_str(include_str!(
            "../../resources/whisper-quantized-models.json"
        ))
        .map_err(|_| AppError::Internal("Invalid embedded Whisper catalog".into()))?;
        if manifest.schema != 1
            || manifest.repository != "ggerganov/whisper.cpp"
            || manifest.revision.len() != 40
            || !manifest.revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(AppError::Internal("Invalid embedded Whisper source".into()));
        }
        let mut models: Vec<_> = WhisperModelSize::ALL
            .into_iter()
            .map(Self::from_size)
            .collect();
        for mut model in manifest.models {
            model.url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                manifest.repository, manifest.revision, model.filename
            );
            models.push(model);
        }
        Ok(models)
    }

    pub fn from_filename(filename: &str) -> Option<Self> {
        Self::all()
            .ok()?
            .into_iter()
            .find(|model| model.filename == filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_quantized_artifacts_have_pinned_source_and_distinct_identity() {
        let models = WhisperArtifact::all().unwrap();
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../resources/whisper-quantized-models.json"
        ))
        .unwrap();
        for baseline in manifest["baselines"].as_array().unwrap() {
            let original = models
                .iter()
                .find(|model| model.variant == baseline["variant"])
                .unwrap();
            assert_eq!(original.bytes, baseline["bytes"].as_u64().unwrap());
            assert_eq!(original.sha256, baseline["sha256"].as_str().unwrap());
            assert_eq!(original.filename, baseline["filename"].as_str().unwrap());
        }
        assert_eq!(models.len(), 10);
        let mut ids = std::collections::HashSet::new();
        let mut filenames = std::collections::HashSet::new();
        for model in &models {
            assert!(ids.insert(&model.variant));
            assert!(filenames.insert(&model.filename));
            assert_eq!(model.sha256.len(), 64);
            assert!(model.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(model.bytes > 0);
            assert!(!model.filename.contains(['/', '\\']));
            assert_eq!(
                WhisperArtifact::from_filename(&model.filename)
                    .unwrap()
                    .id(),
                model.id()
            );
        }
        for model in &models[6..] {
            assert!(model
                .url
                .contains("/5359861c739e955e79d9a303bcbc70fb988958b1/"));
            let selection = super::super::SttModelSelection::from_whisper_path(format!(
                "D:\\models\\{}",
                model.filename
            ));
            assert_eq!(selection.id, model.id());
            selection.validate().unwrap();
            assert!(super::super::SttModelSelection {
                id: model.id(),
                local_path: "/models/ggml-small.bin".into()
            }
            .validate()
            .is_err());
        }
        assert!(WhisperArtifact::from_filename("../ggml-small-q5_1.bin").is_none());
    }
}
