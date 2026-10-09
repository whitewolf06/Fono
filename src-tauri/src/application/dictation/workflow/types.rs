//! Public workflow choices contain no provider addresses or credentials.
use crate::types::{AiMode, Settings};
use fono_core::OperationSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingWorkflow {
    #[default]
    Automatic,
    Manual,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextPreset {
    Raw,
    #[default]
    Clean,
    Format,
    Task,
    Formal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationLanguage {
    En,
    Ru,
    De,
    Fr,
    Es,
}

impl TranslationLanguage {
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ru => "Russian",
            Self::De => "German",
            Self::Fr => "French",
            Self::Es => "Spanish",
        }
    }
}

pub fn effective_preset(settings: &Settings) -> TextPreset {
    settings
        .processing_preset
        .unwrap_or(if settings.ai_mode == AiMode::Format {
            TextPreset::Format
        } else {
            TextPreset::Clean
        })
}

pub fn should_process(settings: &Settings) -> bool {
    settings.ai_mode != AiMode::Off
}
pub fn should_defer(settings: &Settings) -> bool {
    should_process(settings) && settings.processing_workflow == ProcessingWorkflow::Manual
}
pub fn effective_language(settings: &Settings) -> Option<TranslationLanguage> {
    settings
        .processing_translation_enabled
        .then_some(settings.processing_target_language)
        .flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingPhase {
    AwaitingAction,
    Processing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDictation {
    pub session_id: u64,
    pub phase: PendingPhase,
    pub original_text: String,
    pub result_text: Option<String>,
    pub created_at: String,
    pub preset: TextPreset,
    pub target_language: Option<TranslationLanguage>,
    pub processing_enabled: bool,
    pub translation_enabled: bool,
    pub source: OperationSource,
    pub error: Option<String>,
    pub insertion_blocked: bool,
    pub copy_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingAction {
    InsertRaw,
    ProcessAndInsert,
    ProcessPreview,
    Complete,
    Cancel,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRequest {
    pub session_id: u64,
    pub action: PendingAction,
    pub preset: Option<TextPreset>,
    // Some(None) explicitly clears translation; omission preserves the choice.
    #[serde(default, deserialize_with = "nullable_override")]
    pub target_language: Option<Option<TranslationLanguage>>,
}

fn nullable_override<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<TranslationLanguage>>, D::Error> {
    Option::<TranslationLanguage>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_null_clears_translation_but_omission_preserves_it() {
        let absent: PendingRequest =
            serde_json::from_str(r#"{"sessionId":1,"action":"insert_raw"}"#).unwrap();
        let clear: PendingRequest =
            serde_json::from_str(r#"{"sessionId":1,"action":"insert_raw","targetLanguage":null}"#)
                .unwrap();
        assert_eq!(absent.target_language, None);
        assert_eq!(clear.target_language, Some(None));
    }
    #[test]
    fn legacy_format_preserves_its_preset_without_changing_the_workflow() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({"ai_mode":"format"})).unwrap();
        assert_eq!(settings.processing_workflow, ProcessingWorkflow::Automatic);
        assert_eq!(effective_preset(&settings), TextPreset::Format);
        assert_eq!(settings.processing_target_language, None);
    }
    #[test]
    fn processing_off_disables_manual_choice_and_stored_translation() {
        let settings = Settings {
            ai_mode: AiMode::Off,
            processing_workflow: ProcessingWorkflow::Manual,
            processing_target_language: Some(TranslationLanguage::En),
            ..Settings::default()
        };
        assert!(!should_defer(&settings));
        assert!(!should_process(&settings));
        let enabled = Settings {
            ai_mode: AiMode::Clean,
            ..settings
        };
        assert!(should_defer(&enabled));
        assert!(should_process(&enabled));
    }
}
