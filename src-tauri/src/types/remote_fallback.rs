//! Resolve an installed CPU reserve without changing persisted source preferences.
use super::{AccelerationMode, ModelId, Settings, SttEngine, SttModelSelection, SttSource};
use crate::error::{AppError, AppResult};
use std::path::Path;

impl Settings {
    /// Blocking filesystem lookup only. Loading remains with the shared scheduler.
    pub(crate) fn local_fallback_settings(&self, model: Option<&ModelId>) -> AppResult<Self> {
        self.local_fallback_settings_in(model, &crate::state::models_dir()?)
    }

    fn local_fallback_settings_in(
        &self,
        model: Option<&ModelId>,
        directory: &Path,
    ) -> AppResult<Self> {
        let configured = self.selected_stt_model().filter(|selection| {
            model.map_or(true, |id| selection.id == *id) && installed(selection)
        });
        let selection = if let Some(selection) = configured {
            selection
        } else if let Some(id) = model {
            fono_stt::models::catalog::list_stt_models(directory)?
                .into_iter()
                .find(|entry| entry.id == *id && entry.local_path.is_some())
                .and_then(|entry| {
                    entry.local_path.map(|local_path| SttModelSelection {
                        id: entry.id,
                        local_path,
                    })
                })
                .ok_or(AppError::ModelNotSelected)?
        } else {
            return Err(AppError::ModelNotSelected);
        };
        if !installed(&selection) {
            return Err(AppError::ModelNotSelected);
        }
        super::validate_stt_selection(&selection)?;
        let mut settings = self.clone();
        settings.stt_source = SttSource::Local;
        settings.stt_model = Some(selection.clone());
        settings.whisper_model_path =
            (selection.id.engine == SttEngine::Whisper).then_some(selection.local_path);
        // A server reserve never starts a local GPU worker or changes user preferences.
        settings.acceleration = AccelerationMode::Cpu;
        settings.stt_fallback = true;
        settings.pretranscription_enabled = false;
        settings.validate_stt_preferences()?;
        Ok(settings)
    }
}

fn installed(selection: &SttModelSelection) -> bool {
    if selection.id.engine == SttEngine::Whisper {
        return std::fs::metadata(&selection.local_path)
            .is_ok_and(|file| file.is_file() && file.len() > 0);
    }
    #[cfg(feature = "offline-stt")]
    if let Ok(spec) = crate::stt::offline_models::specification(&selection.id) {
        return fono_stt::models::offline_package::is_installed(
            Path::new(&selection.local_path),
            &spec,
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RemoteFallback, RemoteSttSettings, WhisperModelSize};

    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("fono-fallback-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.path().join("ggml-tiny.bin"));
            std::fs::remove_dir(self.path()).unwrap();
        }
    }

    fn remote() -> Settings {
        Settings {
            stt_source: SttSource::FonoServer,
            remote_stt: Some(RemoteSttSettings {
                url: "http://127.0.0.1:17833".into(),
                display_name: None,
                model: ModelId::whisper(WhisperModelSize::LargeTurbo),
                ca_pem: None,
                fallback: RemoteFallback::None,
            }),
            acceleration: AccelerationMode::Cuda,
            ..Default::default()
        }
    }

    #[test]
    fn policy_migration_roundtrip_and_runtime_marker_are_separate() {
        let legacy = serde_json::json!({"url":"http://127.0.0.1:17833", "model":{"engine":"whisper","variant":"tiny"}});
        let decoded: RemoteSttSettings = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.fallback, RemoteFallback::None);
        for policy in [
            RemoteFallback::None,
            RemoteFallback::Ask,
            RemoteFallback::Local(ModelId::whisper(WhisperModelSize::Tiny)),
        ] {
            let mut settings = remote();
            settings.remote_stt.as_mut().unwrap().fallback = policy.clone();
            settings.stt_fallback = true;
            let encoded = serde_json::to_value(&settings).unwrap();
            assert!(encoded.get("stt_fallback").is_none());
            let restored: Settings = serde_json::from_value(encoded).unwrap();
            assert_eq!(restored.remote_stt.unwrap().fallback, policy);
            assert!(!restored.stt_fallback);
        }
    }

    #[test]
    fn cpu_reserve_uses_exact_installed_identity_and_preserves_saved_preferences() {
        let directory = TestDirectory::new();
        let path = directory.path().join("ggml-tiny.bin");
        std::fs::write(&path, [1]).unwrap();
        let selection = SttModelSelection::from_whisper_path(path.to_string_lossy().into_owned());
        let mut saved = remote();
        saved.stt_model = Some(selection.clone());
        let before = serde_json::to_value(&saved).unwrap();
        let local = saved
            .local_fallback_settings_in(None, directory.path())
            .unwrap();
        assert_eq!(local.stt_source, SttSource::Local);
        assert_eq!(local.acceleration, AccelerationMode::Cpu);
        assert_eq!(local.stt_model, Some(selection));
        assert!(local.stt_fallback);
        assert_eq!(serde_json::to_value(&saved).unwrap(), before);
        // Different requested model cannot silently use the existing tiny model.
        assert!(saved
            .local_fallback_settings_in(
                Some(&ModelId::whisper(WhisperModelSize::Small)),
                directory.path()
            )
            .is_err());
        // Explicit installed catalog identity also works with no dormant local model.
        saved.stt_model = None;
        assert!(saved
            .local_fallback_settings_in(
                Some(&ModelId::whisper(WhisperModelSize::Tiny)),
                directory.path()
            )
            .is_ok());
        assert!(saved
            .local_fallback_settings_in(None, directory.path())
            .is_err());
    }

    #[test]
    fn missing_reserve_does_not_download_or_change_the_remote_configuration() {
        let directory = TestDirectory::new();
        let saved = remote();
        assert!(saved
            .local_fallback_settings_in(
                Some(&ModelId::whisper(WhisperModelSize::Tiny)),
                directory.path()
            )
            .is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        assert_eq!(saved.stt_source, SttSource::FonoServer);
    }
}
