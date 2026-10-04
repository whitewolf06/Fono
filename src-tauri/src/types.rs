//! Типы, общие между Rust и TypeScript.
//!
//! Эти структуры зеркалируют `src/lib/types.ts` на фронтенде.
//! При изменении не забудьте синхронизировать обе стороны.

pub use crate::application::dictation::workflow::{
    ProcessingWorkflow, TextPreset, TranslationLanguage,
};
pub use fono_wake::WakeWordBackend;
mod history_metadata;
pub use history_metadata::{
    DictationBackend, DictationHistoryMetadata, DictationTimingMeasurements,
};
use serde::{Deserialize, Serialize};

/// Состояние голосового конвейера (FSM).
/// См. `docs/architecture.md` → "Основной конвейер".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    Idle,
    Listening,
    Transcribing,
    Processing,
    #[serde(rename = "awaiting_action")]
    AwaitingAction,
    Injecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    /// Full-size multilingual Whisper Large v3 model.
    Large,
    /// Faster distilled variant of Large v3. It trades some accuracy for
    /// noticeably lower memory use and latency.
    LargeTurbo,
}

impl WhisperModelSize {
    pub const ALL: [Self; 6] = [
        Self::Tiny,
        Self::Base,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::LargeTurbo,
    ];

    pub fn filename(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "ggml-tiny.bin",
            WhisperModelSize::Base => "ggml-base.bin",
            WhisperModelSize::Small => "ggml-small.bin",
            WhisperModelSize::Medium => "ggml-medium.bin",
            WhisperModelSize::Large => "ggml-large-v3.bin",
            WhisperModelSize::LargeTurbo => "ggml-large-v3-turbo.bin",
        }
    }

    /// Stable public identifier used by the local transcription API.
    pub fn api_identifier(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "tiny",
            WhisperModelSize::Base => "base",
            WhisperModelSize::Small => "small",
            WhisperModelSize::Medium => "medium",
            WhisperModelSize::Large => "large",
            WhisperModelSize::LargeTurbo => "large_turbo",
        }
    }

    pub fn from_filename(filename: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|model| model.filename() == filename)
    }

    pub fn url(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
            }
            WhisperModelSize::Base => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
            }
            WhisperModelSize::Small => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
            }
            WhisperModelSize::Medium => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin"
            }
            WhisperModelSize::Large => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin"
            }
            WhisperModelSize::LargeTurbo => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
            }
        }
    }

    pub fn approx_bytes(&self) -> u64 {
        (match self {
            WhisperModelSize::Tiny => 77,
            WhisperModelSize::Base => 147,
            WhisperModelSize::Small => 488,
            WhisperModelSize::Medium => 1530,
            WhisperModelSize::Large => 3010,
            WhisperModelSize::LargeTurbo => 1549,
        }) * 1024
            * 1024
    }

    /// SHA-256 published for the exact files behind the Hugging Face URLs.
    /// Source: https://huggingface.co/ggerganov/whisper.cpp/tree/main
    pub fn sha256(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => {
                "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
            }
            WhisperModelSize::Base => {
                "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
            }
            WhisperModelSize::Small => {
                "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
            }
            WhisperModelSize::Medium => {
                "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"
            }
            WhisperModelSize::Large => {
                "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2"
            }
            WhisperModelSize::LargeTurbo => {
                "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
            }
        }
    }
}

#[cfg(test)]
mod whisper_model_size_tests {
    use super::WhisperModelSize;

    #[test]
    fn model_catalog_contains_the_two_large_v3_variants() {
        assert_eq!(WhisperModelSize::ALL.len(), 6);
        assert_eq!(WhisperModelSize::Large.filename(), "ggml-large-v3.bin");
        assert_eq!(
            WhisperModelSize::LargeTurbo.filename(),
            "ggml-large-v3-turbo.bin"
        );
        assert!(
            WhisperModelSize::Large.approx_bytes() > WhisperModelSize::LargeTurbo.approx_bytes()
        );
    }

    #[test]
    fn large_turbo_uses_a_stable_ipc_value_and_verification_hash() {
        assert_eq!(
            serde_json::to_string(&WhisperModelSize::LargeTurbo).unwrap(),
            "\"large_turbo\""
        );
        assert_eq!(
            serde_json::from_str::<WhisperModelSize>("\"large\"").unwrap(),
            WhisperModelSize::Large
        );
        assert_eq!(
            WhisperModelSize::LargeTurbo.sha256(),
            "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
        );
    }

    #[test]
    fn public_api_identifiers_are_derived_from_known_model_filenames() {
        assert_eq!(
            WhisperModelSize::from_filename("ggml-large-v3-turbo.bin")
                .map(|model| model.api_identifier()),
            Some("large_turbo")
        );
        assert!(WhisperModelSize::from_filename("custom-model.bin").is_none());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperModelInfo {
    pub filename: String,
    pub size: WhisperModelSize,
    /// `Some(path)` если модель уже скачана локально.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

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

/// Preferred Whisper acceleration. `Auto` uses the GPU backend compiled into
/// this release (CUDA or Vulkan) and falls back to CPU when there is none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccelerationMode {
    #[default]
    Auto,
    Cuda,
    Vulkan,
    Cpu,
}

impl AccelerationMode {
    pub fn use_gpu(self) -> bool {
        match self {
            Self::Cpu => false,
            Self::Auto => cfg!(any(feature = "cuda", feature = "vulkan")),
            Self::Cuda => cfg!(feature = "cuda"),
            Self::Vulkan => cfg!(feature = "vulkan"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AccelerationCapabilities {
    pub cuda: bool,
    pub vulkan: bool,
}

pub fn acceleration_capabilities() -> AccelerationCapabilities {
    AccelerationCapabilities {
        cuda: cfg!(feature = "cuda"),
        vulkan: cfg!(feature = "vulkan"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    LmStudio,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub dictation_mode: DictationMode,
    #[serde(default)]
    pub audio_device_id: Option<String>,
    #[serde(default)]
    pub whisper_model_path: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    /// Exact local canonical spellings; disabled by default, entries retained.
    #[serde(default)]
    pub personal_dictionary_enabled: bool,
    #[serde(default)]
    pub personal_dictionary_entries: Vec<PersonalDictionaryEntry>,
    /// Automatic network checks are optional; explicit checks remain available.
    #[serde(default)]
    pub update_checks_enabled: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub hotkey_mode: HotkeyMode,
    #[serde(default)]
    pub wake_word_enabled: bool,
    #[serde(default = "default_wake_word")]
    pub wake_word: String,
    #[serde(default = "default_wake_backend")]
    pub wake_backend: WakeWordBackend,
    #[serde(default = "default_wake_word_threshold")]
    pub wake_word_threshold: f32,
    #[serde(default = "default_wake_word_sensitivity")]
    pub wake_word_sensitivity: f32,
    /// The completed local calibration profile. It contains aggregate signal
    /// metrics only; raw microphone samples never leave the calibration flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_calibration_profile: Option<WakeCalibrationProfile>,
    #[serde(default = "default_ai_mode")]
    pub ai_mode: AiMode,
    #[serde(default)]
    pub processing_workflow: ProcessingWorkflow,
    /// None retains the legacy ai_mode choice until a new renderer selects a preset.
    #[serde(default)]
    pub processing_preset: Option<TextPreset>,
    #[serde(default)]
    pub processing_target_language: Option<TranslationLanguage>,
    #[serde(default = "default_llm_url")]
    pub llm_base_url: String,
    #[serde(default)]
    pub llm_model: Option<String>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_overlay_enabled")]
    pub service_enabled: bool,
    #[serde(default)]
    pub overlay_x: Option<i32>,
    #[serde(default)]
    pub overlay_y: Option<i32>,
    #[serde(default)]
    pub clean_prompt: Option<String>,
    /// Legacy field read from existing settings.json files. New settings use
    /// `acceleration`; keep it out of IPC and future writes.
    #[serde(skip_serializing, default = "default_use_gpu")]
    pub use_gpu: bool,
    #[serde(default)]
    pub acceleration: AccelerationMode,
    #[serde(default = "default_injection_mode")]
    pub injection_mode: InjectionMode,
    #[serde(default = "default_command_hotkey")]
    pub command_hotkey: String,
    #[serde(default)]
    pub launch_apps: Vec<LaunchApp>,
    #[serde(default = "default_overlay_enabled")]
    pub overlay_enabled: bool,
    #[serde(default = "default_overlay_scale")]
    pub overlay_scale: f32,
    #[serde(default = "default_overlay_opacity")]
    pub overlay_opacity: f32,
    #[serde(default)]
    pub overlay_mini_mode: bool,
    #[serde(default)]
    pub verbose_logging: bool,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: LlmProvider,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub llm_api_key: Option<String>,
    #[serde(default)]
    pub has_llm_api_key: bool,
    /// Named LLM connections. The legacy fields above are kept only to read
    /// existing settings and mirror the default profile for the old UI.
    #[serde(default)]
    pub llm_profiles: Vec<LlmProfile>,
    #[serde(default)]
    pub text_correction_llm: LlmConsumerAssignment,
    #[serde(default)]
    pub speech_analysis_llm: SpeechLlmAssignment,
    #[serde(default = "default_history_enabled")]
    pub history_enabled: bool,
    /// Explicit consent for keeping the original transcript and processing
    /// metadata locally for future speech analytics. Disabled by default.
    #[serde(default)]
    pub analytics_enabled: bool,
    /// Pauses collection and analysis of new speech trainer sessions without
    /// deleting locally retained data. Defaults to enabled for existing users.
    #[serde(default = "default_speech_trainer_enabled")]
    pub speech_trainer_enabled: bool,
    /// How long locally stored analytics payload may remain attached to a
    /// history entry. The final inserted text keeps the normal history policy.
    #[serde(default = "default_analytics_retention_days")]
    pub analytics_retention_days: u16,
    #[serde(default = "default_wake_word_model")]
    pub wake_word_model: WhisperModelSize,
    #[serde(default = "default_wake_word_vad_threshold")]
    pub wake_word_vad_threshold: f32,
    /// Сколько тишины после речи ждать перед завершением диктовки по wake word.
    #[serde(default = "default_wake_dictation_silence_ms")]
    pub wake_dictation_silence_ms: u64,
    /// RMS-порог, ниже которого диктовка после wake word считает звук тишиной.
    #[serde(default = "default_wake_dictation_speech_threshold")]
    pub wake_dictation_speech_threshold: f32,
    #[serde(default = "default_volume_step")]
    pub volume_step: u32,
}

/// Persisted result of a completed local wake-word calibration. This is
/// descriptive metadata for FONO-45 to validate and activate later; FONO-44
/// deliberately does not alter the current wake threshold automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCalibrationProfile {
    pub backend: WakeWordBackend,
    pub model_version: String,
    pub phrase: String,
    pub graph: String,
    pub threshold: f32,
    pub sensitivity: f32,
    pub vad_threshold: f32,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub accepted_samples: u8,
    pub rejected_samples: u8,
    pub average_rms: f32,
    pub average_peak: f32,
    pub average_active_ms: u64,
    /// Present only after a separate, fresh validation session succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<WakeCalibrationValidation>,
}

/// Aggregate outcome of the post-registration validation. It deliberately
/// contains counts, not recordings, transcripts, keywords, or a made-up
/// accuracy percentage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCalibrationValidation {
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub positive_passed: u8,
    pub positive_required: u8,
    pub negative_passed: u8,
    pub negative_required: u8,
    pub confirmed_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchApp {
    pub name: String,
    pub exe_path: String,
    pub aliases: Vec<String>,
}

/// Immutable settings required to execute a confirmed voice-command proposal.
/// Secrets and unrelated UI settings are deliberately excluded.
#[derive(Debug, Clone)]
pub struct CommandSettingsSnapshot {
    pub version: u64,
    pub launch_apps: Vec<LaunchApp>,
    pub volume_step: u32,
}

/// A command is always previewed before it may affect another application.
/// The payload is serialisable so the renderer can show the proposal without
/// reaching into application state.
#[derive(Debug, Clone, Serialize)]
pub struct CommandProposal {
    pub id: u64,
    pub operation_id: u64,
    pub source: crate::operation::OperationSource,
    pub original_text: String,
    pub normalized_action: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub settings_version: u64,
    #[serde(skip)]
    pub settings_snapshot: CommandSettingsSnapshot,
}

impl CommandProposal {
    pub fn is_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now >= self.expires_at
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dictation_mode: DictationMode::Standard,
            audio_device_id: None,
            whisper_model_path: None,
            language: default_language(),
            personal_dictionary_enabled: false,
            personal_dictionary_entries: Vec::new(),
            update_checks_enabled: false,
            hotkey: default_hotkey(),
            hotkey_mode: HotkeyMode::Hold,
            wake_word_enabled: false,
            wake_word: default_wake_word(),
            wake_backend: default_wake_backend(),
            wake_word_threshold: default_wake_word_threshold(),
            wake_word_sensitivity: default_wake_word_sensitivity(),
            wake_calibration_profile: None,
            ai_mode: default_ai_mode(),
            processing_workflow: ProcessingWorkflow::Automatic,
            processing_preset: None,
            processing_target_language: None,
            llm_base_url: default_llm_url(),
            llm_model: None,
            autostart: false,
            service_enabled: true,
            overlay_x: None,
            overlay_y: None,
            clean_prompt: None,
            use_gpu: default_use_gpu(),
            acceleration: AccelerationMode::Auto,
            injection_mode: default_injection_mode(),
            command_hotkey: default_command_hotkey(),
            launch_apps: Vec::new(),
            overlay_enabled: true,
            overlay_scale: default_overlay_scale(),
            overlay_opacity: default_overlay_opacity(),
            overlay_mini_mode: false,
            verbose_logging: false,
            llm_provider: default_llm_provider(),
            llm_api_key: None,
            has_llm_api_key: false,
            llm_profiles: vec![LlmProfile {
                id: LlmProfile::DEFAULT_ID.to_owned(),
                name: "Основной LLM".to_owned(),
                provider: default_llm_provider(),
                connection: LlmConnectionKind::Local,
                base_url: default_llm_url(),
                model: None,
                api_key: None,
                has_api_key: false,
            }],
            text_correction_llm: LlmConsumerAssignment::default(),
            speech_analysis_llm: SpeechLlmAssignment::default(),
            history_enabled: default_history_enabled(),
            analytics_enabled: false,
            speech_trainer_enabled: default_speech_trainer_enabled(),
            analytics_retention_days: default_analytics_retention_days(),
            wake_word_model: default_wake_word_model(),
            wake_word_vad_threshold: default_wake_word_vad_threshold(),
            wake_dictation_silence_ms: default_wake_dictation_silence_ms(),
            wake_dictation_speech_threshold: default_wake_dictation_speech_threshold(),
            volume_step: default_volume_step(),
        }
    }
}

impl Settings {
    /// Live dictation is temporarily unavailable after manual quality review.
    /// Normalize both persisted settings and settings submitted by older clients.
    pub fn enforce_classic_dictation(&mut self) -> bool {
        let changed = self.dictation_mode != DictationMode::Standard;
        self.dictation_mode = DictationMode::Standard;
        changed
    }

    /// Returns true when an old single-connection configuration was migrated.
    pub fn migrate_llm_profiles(&mut self) -> bool {
        if !self.llm_profiles.is_empty() {
            return false;
        }
        self.llm_profiles.push(LlmProfile::legacy_default(self));
        if self.text_correction_llm.profile_id.is_none() {
            self.text_correction_llm = LlmConsumerAssignment::default();
        }
        true
    }

    pub fn llm_profile(&self, id: Option<&str>) -> Option<&LlmProfile> {
        let id = id?;
        self.llm_profiles.iter().find(|profile| profile.id == id)
    }

    pub fn correction_profile(&self) -> Option<&LlmProfile> {
        self.llm_profile(self.text_correction_llm.profile_id.as_deref())
    }

    pub fn speech_analysis_profile(&self) -> Option<&LlmProfile> {
        self.llm_profile(self.speech_analysis_llm.profile_id.as_deref())
    }
}

#[cfg(test)]
mod wake_calibration_settings_tests {
    use super::{AiMode, DictationMode, HotkeyMode, Settings};

    #[test]
    fn legacy_settings_default_to_hold_and_toggle_round_trips() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({})).expect("legacy settings deserialize");
        assert_eq!(settings.hotkey_mode, HotkeyMode::Hold);
        let settings = Settings {
            hotkey_mode: HotkeyMode::Toggle,
            ..settings
        };
        let json = serde_json::to_value(&settings).expect("serialize hotkey mode");
        assert_eq!(json["hotkey_mode"], "toggle");
        let restored: Settings = serde_json::from_value(json).expect("restore hotkey mode");
        assert_eq!(restored.hotkey_mode, HotkeyMode::Toggle);
    }

    #[test]
    fn saved_live_mode_returns_to_classic_without_losing_processing_preferences() {
        let mut settings: Settings = serde_json::from_value(serde_json::json!({
            "dictation_mode": "live",
            "ai_mode": "format",
            "language": "en",
            "clean_prompt": "Keep this instruction"
        }))
        .expect("previous live settings deserialize");

        assert!(settings.enforce_classic_dictation());
        assert_eq!(settings.dictation_mode, DictationMode::Standard);
        assert_eq!(settings.ai_mode, AiMode::Format);
        assert_eq!(settings.language, "en");
        assert_eq!(
            settings.clean_prompt.as_deref(),
            Some("Keep this instruction")
        );
        assert!(!settings.enforce_classic_dictation());
    }

    #[test]
    fn legacy_settings_without_calibration_profile_remain_compatible() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "wake_word": "рамзи",
            "wake_backend": "sherpa_onnx"
        }))
        .expect("legacy settings deserialize");

        assert!(settings.wake_calibration_profile.is_none());
    }
}

fn default_use_gpu() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

fn default_history_enabled() -> bool {
    true
}

fn default_speech_trainer_enabled() -> bool {
    true
}

fn default_analytics_retention_days() -> u16 {
    30
}

fn default_injection_mode() -> InjectionMode {
    InjectionMode::SendInput
}

fn default_command_hotkey() -> String {
    "Ctrl+Shift+Space".to_string()
}

fn default_overlay_scale() -> f32 {
    1.0
}

fn default_overlay_enabled() -> bool {
    true
}

fn default_overlay_opacity() -> f32 {
    1.0
}

fn default_llm_provider() -> LlmProvider {
    LlmProvider::LmStudio
}

fn default_wake_word_model() -> WhisperModelSize {
    WhisperModelSize::Base
}

fn default_wake_backend() -> WakeWordBackend {
    WakeWordBackend::SherpaOnnx
}

fn default_wake_word_threshold() -> f32 {
    0.25
}

fn default_wake_word_sensitivity() -> f32 {
    0.5
}

fn default_wake_word_vad_threshold() -> f32 {
    0.015
}

fn default_wake_dictation_silence_ms() -> u64 {
    2_000
}

fn default_wake_dictation_speech_threshold() -> f32 {
    0.006
}

fn default_volume_step() -> u32 {
    10
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationHistoryEntry {
    pub id: String,
    /// Final text that was inserted. This stays the stable field used by
    /// existing history files and renderer clients.
    pub text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub device: Option<String>,
    /// Content-free session facts survive independently of opt-in trainer data.
    /// Older archives have no measurements and remain readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<DictationHistoryMetadata>,
    /// A user-controlled filter for speech trainer reports and background analysis.
    /// Legacy history entries are included to preserve their existing behaviour.
    #[serde(default = "default_history_analytics_included")]
    pub analytics_included: bool,
    /// Original Whisper output is personal data and is saved only after the
    /// user explicitly enables local speech analytics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<DictationProcessingMetadata>,
    #[serde(default)]
    pub analysis_status: DictationAnalysisStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<SpeechSessionAnalysis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_error: Option<String>,
    /// Optional LLM coaching layer built only after local metrics are ready.
    #[serde(default)]
    pub recommendation_status: DictationAnalysisStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<crate::llm::SpeechLlmRecommendation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationProcessingMetadata {
    pub ai_mode: AiMode,
    pub detected_language: Option<String>,
    pub transcribe_secs: Option<f32>,
    pub audio_secs: Option<f32>,
}

/// Local, explainable results calculated only from a saved opt-in transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechSessionAnalysis {
    pub word_count: u32,
    pub filler_count: u32,
    pub filler_density_per_100_words: f32,
    pub repetition_count: u32,
    pub self_correction_count: u32,
    pub unfinished_count: u32,
    pub findings: Vec<SpeechFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechFinding {
    pub kind: SpeechFindingKind,
    pub label: String,
    pub fragment: String,
    pub start_word: u32,
    pub end_word: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechFindingKind {
    Filler,
    Repetition,
    SelfCorrection,
    Unfinished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechPeriodReport {
    pub from: chrono::DateTime<chrono::Utc>,
    pub to: chrono::DateTime<chrono::Utc>,
    pub analyzed_sessions: u32,
    pub total_words: u32,
    pub filler_count: u32,
    pub repetition_count: u32,
    pub self_correction_count: u32,
    pub unfinished_count: u32,
    pub filler_density_per_100_words: f32,
    pub daily: Vec<SpeechDailyTrend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechDailyTrend {
    pub date: chrono::NaiveDate,
    pub sessions: u32,
    pub words: u32,
    pub filler_count: u32,
    pub repetition_count: u32,
    pub self_correction_count: u32,
    pub unfinished_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DictationAnalysisStatus {
    #[default]
    Disabled,
    Pending,
    Ready,
    Failed,
    Expired,
}

impl DictationHistoryEntry {
    pub fn clear_analytics_data(&mut self, status: DictationAnalysisStatus) -> bool {
        let removed_original_text = self.original_text.take().is_some();
        let removed_processing_metadata = self.processing.take().is_some();
        let removed_analysis = self.analysis.take().is_some();
        let removed_analysis_error = self.analysis_error.take().is_some();
        let removed_recommendation = self.recommendation.take().is_some();
        let removed_recommendation_error = self.recommendation_error.take().is_some();
        let mut changed = removed_original_text
            || removed_processing_metadata
            || removed_analysis
            || removed_analysis_error
            || removed_recommendation
            || removed_recommendation_error;
        if self.analysis_status != status {
            self.analysis_status = status;
            changed = true;
        }
        if self.recommendation_status != status {
            self.recommendation_status = status;
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod dictation_history_tests {
    use super::{DictationAnalysisStatus, DictationHistoryEntry, Settings};

    #[test]
    fn legacy_history_entry_keeps_final_text_and_defaults_private_fields() {
        let entry: DictationHistoryEntry = serde_json::from_value(serde_json::json!({
            "id": "legacy-entry",
            "text": "already inserted text",
            "created_at": "2026-08-31T12:00:00Z",
            "device": "CUDA"
        }))
        .expect("legacy history entry deserializes");

        assert_eq!(entry.text, "already inserted text");
        assert!(entry.original_text.is_none());
        assert!(entry.processing.is_none());
        assert_eq!(entry.analysis_status, DictationAnalysisStatus::Disabled);
        assert!(entry.analytics_included);
    }

    #[test]
    fn legacy_settings_default_to_disabled_analytics() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({})).expect("legacy settings deserialize");

        assert!(!settings.analytics_enabled);
        assert!(settings.speech_trainer_enabled);
        assert_eq!(settings.analytics_retention_days, 30);
    }

    #[test]
    fn legacy_llm_settings_migrate_to_a_named_default_profile() {
        let mut settings = Settings {
            llm_base_url: "http://localhost:9999/v1".into(),
            llm_model: Some("local-model".into()),
            ..Settings::default()
        };
        settings.llm_profiles.clear();

        assert!(settings.migrate_llm_profiles());
        let profile = settings.correction_profile().expect("default profile");
        assert_eq!(profile.id, "default");
        assert_eq!(profile.base_url, "http://localhost:9999/v1");
        assert_eq!(profile.model.as_deref(), Some("local-model"));
    }

    #[test]
    fn api_keys_are_not_serialized_to_settings_json() {
        let mut settings = Settings::default();
        settings.llm_profiles[0].api_key = Some("secret".into());
        settings.llm_api_key = Some("legacy-secret".into());
        let json = serde_json::to_value(&settings).expect("serialize settings");

        assert!(json.get("llm_api_key").is_none());
        assert!(json["llm_profiles"][0].get("api_key").is_none());
        assert!(!json.to_string().contains("secret"));
    }
}

fn default_language() -> String {
    "auto".to_string()
}
fn default_hotkey() -> String {
    "Ctrl+Space".to_string()
}
fn default_wake_word() -> String {
    "hey fono".to_string()
}
fn default_ai_mode() -> AiMode {
    AiMode::Clean
}
fn default_history_analytics_included() -> bool {
    true
}
fn default_llm_url() -> String {
    "http://localhost:1234/v1".to_string()
}
