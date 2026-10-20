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
        let (options, fixed_language) = match self.selected_stt_model() {
            Some(model)
                if model.id.engine == SttEngine::Whisper && model.id.whisper_artifact().is_ok() =>
            {
                (vec!["ru".into(), "en".into(), "auto".into()], None)
            }
            Some(model) => match crate::stt::offline_models::specification(&model.id) {
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
