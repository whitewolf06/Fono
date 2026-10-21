//! Processing configuration and model connection contracts.

use super::{defaults::*, Settings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMode {
    Off,
    Clean,
    Format,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InjectionMode {
    SendInput,
    Clipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    LmStudio,
    Ollama,
    OpenAi,
    Custom,
}

/// Where the selected OpenAI-compatible endpoint runs. This is deliberately
/// separate from the protocol provider: a custom endpoint can still be local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmConnectionKind {
    #[default]
    Local,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmProfile {
    /// Stable, URL-safe identifier. It is also part of the OS credential name.
    pub id: String,
    pub name: String,
    #[serde(default = "default_llm_provider")]
    pub provider: LlmProvider,
    #[serde(default)]
    pub connection: LlmConnectionKind,
    #[serde(default = "default_llm_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Never serialized and never returned to the renderer after saving.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub has_api_key: bool,
}

impl LlmProfile {
    pub const DEFAULT_ID: &'static str = "default";

    pub fn legacy_default(settings: &Settings) -> Self {
        Self {
            id: Self::DEFAULT_ID.to_owned(),
            name: "Основной LLM".to_owned(),
            provider: settings.llm_provider,
            connection: if matches!(settings.llm_provider, LlmProvider::OpenAi) {
                LlmConnectionKind::Cloud
            } else {
                LlmConnectionKind::Local
            },
            base_url: settings.llm_base_url.clone(),
            model: settings.llm_model.clone(),
            api_key: settings.llm_api_key.clone(),
            has_api_key: settings.has_llm_api_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmConsumerAssignment {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for LlmConsumerAssignment {
    fn default() -> Self {
        Self {
            profile_id: Some(LlmProfile::DEFAULT_ID.to_owned()),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpeechLlmDataScope {
    /// Send only numeric local metrics; never include transcript fragments.
    #[default]
    MetricsOnly,
    /// Send locally detected findings and their short fragments.
    Findings,
    /// Send the complete original transcript. This requires separate consent.
    OriginalText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechLlmAssignment {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub data_scope: SpeechLlmDataScope,
    /// Explicit permission to send the chosen scope to a cloud profile.
    #[serde(default)]
    pub cloud_consent: bool,
}

impl Default for SpeechLlmAssignment {
    fn default() -> Self {
        Self {
            enabled: false,
            profile_id: None,
            model: None,
            data_scope: SpeechLlmDataScope::MetricsOnly,
            cloud_consent: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationMode {
    #[default]
    Standard,
    Live,
}

/// Global dictation shortcut behavior; legacy settings retain push-to-talk.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    #[default]
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalDictionaryEntry {
    pub written: String,
    pub spoken: Vec<String>,
}
