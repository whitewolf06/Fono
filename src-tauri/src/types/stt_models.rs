//! Stable speech-model identity, independent of a backend's local file path.
use super::{Settings, WhisperModelSize};
use crate::error::{AppError, AppResult};
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
    pub fn whisper(size: WhisperModelSize) -> Self {
        Self {
            engine: SttEngine::Whisper,
            variant: size.api_identifier().into(),
        }
    }

    /// Other backends acquire runnable artifacts in F-01.3; never route them
    /// through the Whisper loader merely because their files exist.
    pub fn whisper_size(&self) -> AppResult<WhisperModelSize> {
        if self.engine != SttEngine::Whisper {
            return Err(AppError::Config(format!(
                "STT engine {:?} is not available in this build",
                self.engine
            )));
        }
        WhisperModelSize::ALL
            .into_iter()
            .find(|size| size.api_identifier() == self.variant)
            .ok_or_else(|| AppError::Config(format!("Unknown Whisper model: {}", self.variant)))
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
        let id = WhisperModelSize::from_filename(filename)
            .map(ModelId::whisper)
            .unwrap_or(ModelId {
                engine: SttEngine::Whisper,
                variant: "custom".into(),
            });
        Self {
            id,
            local_path: path,
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.local_path.trim().is_empty() {
            return Err(AppError::Config("STT model path is empty".into()));
        }
        if self.id.engine == SttEngine::Whisper && self.id.variant == "custom" {
            return Ok(());
        }
        let size = self.id.whisper_size()?;
        let filename = self.local_path.rsplit(['/', '\\']).next();
        if filename != Some(size.filename()) {
            return Err(AppError::Config(
                "STT model identity does not match its file".into(),
            ));
        }
        Ok(())
    }
}

impl Settings {
    /// New selections are authoritative. A writer changing only the legacy
    /// path is still supported; update both fields as one settings delta.
    pub fn reconcile_stt_model(&mut self, previous: Option<&Settings>) -> AppResult<bool> {
        let before = (self.stt_model.clone(), self.whisper_model_path.clone());
        if previous.is_some_and(|old| {
            old.whisper_model_path != self.whisper_model_path && old.stt_model == self.stt_model
        }) {
            self.stt_model = self
                .whisper_model_path
                .clone()
                .map(SttModelSelection::from_whisper_path);
        }
        if self.stt_model.is_none() {
            self.stt_model = self
                .whisper_model_path
                .clone()
                .map(SttModelSelection::from_whisper_path);
        }
        if let Some(selection) = &self.stt_model {
            selection.validate()?;
            self.whisper_model_path = Some(selection.local_path.clone());
        }
        Ok(before != (self.stt_model.clone(), self.whisper_model_path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_known_and_custom_paths_survive_migration_and_roundtrip() {
        for (path, variant) in [
            (r"D:\models\ggml-large-v3-turbo.bin", "large_turbo"),
            ("/models/my-own-q5.bin", "custom"),
        ] {
            let mut settings: Settings = serde_json::from_value(serde_json::json!({
                "whisper_model_path": path
            }))
            .unwrap();
            assert!(settings.reconcile_stt_model(None).unwrap());
            assert_eq!(settings.stt_model.as_ref().unwrap().id.variant, variant);
            assert_eq!(settings.whisper_model_path.as_deref(), Some(path));
            let mut restored: Settings =
                serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
            assert!(!restored.reconcile_stt_model(None).unwrap());
            assert_eq!(restored.stt_model, settings.stt_model);
        }
    }

    #[test]
    fn new_selection_and_legacy_writer_both_keep_compatibility_path_in_sync() {
        let mut settings = Settings {
            stt_model: Some(SttModelSelection::from_whisper_path(
                "/models/ggml-small.bin".into(),
            )),
            ..Settings::default()
        };
        settings.reconcile_stt_model(None).unwrap();
        assert_eq!(
            settings.whisper_model_path.as_deref(),
            Some("/models/ggml-small.bin")
        );
        let previous = settings.clone();
        settings.whisper_model_path = Some("/models/ggml-base.bin".into());
        settings.reconcile_stt_model(Some(&previous)).unwrap();
        assert_eq!(
            settings.stt_model.as_ref().unwrap().id,
            ModelId::whisper(WhisperModelSize::Base)
        );
        let previous = settings.clone();
        settings.whisper_model_path = None;
        settings.reconcile_stt_model(Some(&previous)).unwrap();
        assert!(settings.stt_model.is_none());
    }

    #[test]
    fn unavailable_or_unknown_engines_are_explicit_errors() {
        assert!(serde_json::from_value::<ModelId>(
            serde_json::json!({"engine":"future_engine","variant":"x"})
        )
        .is_err());
        for engine in [SttEngine::GigaAm, SttEngine::Parakeet] {
            let mut settings = Settings {
                stt_model: Some(SttModelSelection {
                    id: ModelId {
                        engine,
                        variant: "v3".into(),
                    },
                    local_path: "/model.onnx".into(),
                }),
                ..Settings::default()
            };
            assert!(settings.reconcile_stt_model(None).is_err());
        }
        assert!(ModelId {
            engine: SttEngine::Whisper,
            variant: "../tiny".into()
        }
        .whisper_size()
        .is_err());
    }

    #[test]
    fn mismatched_known_model_and_file_are_rejected() {
        let selection = SttModelSelection {
            id: ModelId::whisper(WhisperModelSize::Tiny),
            local_path: "/ggml-base.bin".into(),
        };
        assert!(selection.validate().is_err());
    }
}
