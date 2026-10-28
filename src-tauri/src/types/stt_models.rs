//! Desktop settings migration and enabled-backend validation.

use super::Settings;
#[cfg(test)]
use super::WhisperModelSize;
use crate::error::{AppError, AppResult};
pub use fono_types::{ModelId, SttEngine, SttModelSelection};

/// Structural validation is shared; enabled offline registries stay in desktop.
pub fn validate_stt_selection(model: &SttModelSelection) -> AppResult<()> {
    model.validate()?;
    if model.id.engine != SttEngine::Whisper {
        #[cfg(feature = "offline-stt")]
        return crate::stt::offline_models::specification(&model.id).map(|_| ());
        #[cfg(not(feature = "offline-stt"))]
        return Err(AppError::Config(
            "Offline STT is unavailable in this build".into(),
        ));
    }
    Ok(())
}

impl Settings {
    pub fn selected_stt_model(&self) -> Option<SttModelSelection> {
        self.stt_model.clone().or_else(|| {
            self.whisper_model_path
                .clone()
                .map(SttModelSelection::from_whisper_path)
        })
    }

    pub fn validate_stt_preferences(&self) -> AppResult<()> {
        let Some(model) = self.selected_stt_model() else {
            return Ok(());
        };
        validate_stt_selection(&model)?;
        if model.id.engine != SttEngine::Whisper {
            let spec = crate::stt::offline_models::specification(&model.id)?;
            if self.language != "auto" && !spec.languages.contains(&self.language) {
                return Err(AppError::Config(
                    "Выбранная модель не поддерживает этот язык".into(),
                ));
            }
            if !matches!(
                self.acceleration,
                super::AccelerationMode::Auto | super::AccelerationMode::Cpu
            ) {
                return Err(AppError::Config(
                    "GigaAM и Parakeet в этой сборке работают на CPU. Выберите Авто или Процессор."
                        .into(),
                ));
            }
        }
        Ok(())
    }

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
            validate_stt_selection(selection)?;
            self.whisper_model_path =
                (selection.id.engine == SttEngine::Whisper).then(|| selection.local_path.clone());
        }
        Ok(before != (self.stt_model.clone(), self.whisper_model_path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_validation_uses_the_enabled_offline_registry() {
        for spec in crate::stt::offline_models::specifications().unwrap() {
            let selection = SttModelSelection {
                id: spec.id,
                local_path: format!("/models/{}", spec.directory),
            };
            assert!(selection.validate().is_ok());
            assert_eq!(
                validate_stt_selection(&selection).is_ok(),
                cfg!(feature = "offline-stt")
            );
            let unknown = SttModelSelection {
                id: ModelId {
                    variant: "unknown".into(),
                    ..selection.id
                },
                ..selection
            };
            assert!(validate_stt_selection(&unknown).is_err());
        }
    }
    #[cfg(feature = "offline-stt")]
    #[test]
    fn offline_settings_never_mirror_package_directory_to_whisper_and_keep_decoding_preferences() {
        for spec in crate::stt::offline_models::specifications().unwrap() {
            let mut settings = Settings {
                stt_model: Some(SttModelSelection {
                    id: spec.id.clone(),
                    local_path: format!("/models/{}", spec.directory),
                }),
                language: "auto".into(),
                whisper_decoding: super::super::DecodingStrategy::Accurate,
                whisper_threads: Some(4),
                ..Settings::default()
            };
            settings.reconcile_stt_model(None).unwrap();
            assert!(settings.whisper_model_path.is_none());
            assert_eq!(settings.selected_stt_model().unwrap().id, spec.id);
            assert_eq!(
                settings.decoding_options(),
                super::super::DecodingOptions::default()
            );
            assert_eq!(settings.whisper_threads, Some(4));
            assert!(settings.validate_stt_preferences().is_ok());
            settings.acceleration = super::super::AccelerationMode::Cuda;
            assert!(settings.validate_stt_preferences().is_err());
            settings.acceleration = super::super::AccelerationMode::Auto;
            settings.language = "zz".into();
            assert!(settings.validate_stt_preferences().is_err());
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.stt_model, settings.stt_model);
        }
    }

    #[test]
    fn legacy_known_and_custom_paths_survive_migration_and_roundtrip() {
        for (path, variant) in [
            (r"D:\models\ggml-large-v3-turbo.bin", "large_turbo"),
            (r"D:\models\ggml-small-q5_1.bin", "small_q5_1"),
            ("/models/ggml-large-v3-turbo-q8_0.bin", "large_turbo_q8_0"),
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
