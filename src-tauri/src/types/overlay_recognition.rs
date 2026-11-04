//! Small, path-free capability snapshot for the recording overlay.
use super::{Settings, SttEngine};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayRecognitionCapability {
    pub options: Vec<String>,
    pub fixed_language: Option<String>,
}

impl Settings {
    pub fn overlay_recognition_capability(&self) -> OverlayRecognitionCapability {
        let (options, fixed_language) = match self.recognition_model_id() {
            Some(model)
                if model.engine == SttEngine::Whisper && model.whisper_artifact().is_ok() =>
            {
                (vec!["ru".into(), "en".into(), "auto".into()], None)
            }
            Some(model) => match crate::stt::offline_models::specification(&model) {
                Ok(spec) => (
                    Vec::new(),
                    Some(if spec.languages.len() == 1 {
                        spec.languages[0].clone()
                    } else {
                        "auto".into()
                    }),
                ),
                Err(_) => (Vec::new(), None),
            },
            None => (Vec::new(), None),
        };
        OverlayRecognitionCapability {
            options,
            fixed_language,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelId, SttModelSelection};

    #[test]
    fn overlay_reads_the_active_source_model_and_preserves_the_dormant_local_choice() {
        let settings = Settings {
            whisper_model_path: Some("PRIVATE_DIRECTORY/custom.bin".into()),
            stt_source: crate::types::SttSource::FonoServer,
            remote_stt: Some(crate::types::RemoteSttSettings {
                url: "https://PRIVATE_SERVER.invalid".into(),
                display_name: None,
                model: ModelId {
                    engine: SttEngine::Whisper,
                    variant: "small".into(),
                },
                ca_pem: None,
                fallback: Default::default(),
            }),
            ..Settings::default()
        };
        let capability = settings.overlay_recognition_capability();
        assert_eq!(capability.options, ["ru", "en", "auto"]);
        assert!(!serde_json::to_string(&capability)
            .unwrap()
            .contains("PRIVATE"));
        let local = Settings {
            stt_source: crate::types::SttSource::Local,
            ..settings
        };
        assert_eq!(
            local.whisper_model_path.as_deref(),
            Some("PRIVATE_DIRECTORY/custom.bin")
        );
        assert!(local.overlay_recognition_capability().options.is_empty());
    }

    #[test]
    fn overlay_uses_backend_capability_without_paths_or_forced_offline_languages() {
        let mut settings = Settings::default();
        assert!(settings.overlay_recognition_capability().options.is_empty());
        settings.whisper_model_path = Some("/private/ggml-small.bin".into());
        assert_eq!(
            settings.overlay_recognition_capability().options,
            ["ru", "en", "auto"]
        );
        settings.whisper_model_path = Some("/private/custom.en.bin".into());
        assert!(settings.overlay_recognition_capability().options.is_empty());
        for spec in crate::stt::offline_models::specifications().unwrap() {
            settings.stt_model = Some(SttModelSelection {
                id: spec.id,
                local_path: "/private/package".into(),
            });
            let capability = settings.overlay_recognition_capability();
            assert!(capability.options.is_empty());
            assert_eq!(
                capability.fixed_language.as_deref(),
                Some(if spec.languages.len() == 1 {
                    "ru"
                } else {
                    "auto"
                })
            );
            assert!(!serde_json::to_string(&capability)
                .unwrap()
                .contains("private"));
        }
        settings.stt_model.as_mut().unwrap().id = ModelId {
            engine: SttEngine::GigaAm,
            variant: "unknown".into(),
        };
        assert!(settings
            .overlay_recognition_capability()
            .fixed_language
            .is_none());
    }
}
