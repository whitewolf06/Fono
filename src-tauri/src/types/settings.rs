//! Persisted preferences, compatibility normalization and safe defaults.

use super::{defaults::*, *};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub whisper_decoding: DecodingStrategy,
    #[serde(default)]
    pub whisper_threads: Option<u8>,
    #[serde(default)]
    pub dictation_mode: DictationMode,
    /// Opt-in until long-recording quality has been qualified.
    #[serde(default)]
    pub pretranscription_enabled: bool,
    #[serde(default)]
    pub audio_device_id: Option<String>,
    #[serde(default)]
    pub whisper_model_path: Option<String>,
    /// Canonical model selection; legacy path is retained as a compatibility mirror.
    #[serde(default)]
    pub stt_model: Option<SttModelSelection>,
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
    #[serde(default = "default_processing_translation_enabled")]
    pub processing_translation_enabled: bool,
    #[serde(default)]
    pub processing_prompts: ProcessingPrompts,
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
    #[serde(default)]
    pub gpu_model_residency: GpuModelResidency,
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
    #[serde(default = "default_overlay_enabled")]
    pub overlay_quick_processing: bool,
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
    /// Retain the final dictation result for this many days, with no entry-count cap.
    #[serde(default = "default_history_retention_days")]
    pub history_retention_days: u16,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            whisper_decoding: DecodingStrategy::Fast,
            whisper_threads: None,
            dictation_mode: DictationMode::Standard,
            pretranscription_enabled: false,
            audio_device_id: None,
            whisper_model_path: None,
            stt_model: None,
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
            processing_translation_enabled: true,
            processing_prompts: ProcessingPrompts::default(),
            llm_base_url: default_llm_url(),
            llm_model: None,
            autostart: false,
            service_enabled: true,
            overlay_x: None,
            overlay_y: None,
            clean_prompt: None,
            use_gpu: default_use_gpu(),
            acceleration: AccelerationMode::Auto,
            gpu_model_residency: GpuModelResidency::Resident,
            injection_mode: default_injection_mode(),
            command_hotkey: default_command_hotkey(),
            launch_apps: Vec::new(),
            overlay_enabled: true,
            overlay_scale: default_overlay_scale(),
            overlay_opacity: default_overlay_opacity(),
            overlay_mini_mode: false,
            overlay_quick_processing: true,
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
            history_retention_days: default_history_retention_days(),
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
    pub fn decoding_options(&self) -> DecodingOptions {
        if self
            .selected_stt_model()
            .is_some_and(|model| model.id.engine != super::SttEngine::Whisper)
        {
            // Keep Whisper preferences for switching back; offline models do
            // not implement Whisper's beam search or thread controls.
            return DecodingOptions::default();
        }
        DecodingOptions {
            strategy: self.whisper_decoding,
            threads: self.whisper_threads,
        }
    }
    /// Live dictation is temporarily unavailable after manual quality review.
    /// Normalize both persisted settings and settings submitted by older clients.
    pub fn enforce_classic_dictation(&mut self) -> bool {
        let changed = self.dictation_mode != DictationMode::Standard;
        self.dictation_mode = DictationMode::Standard;
        changed
    }

    /// Keep the phrase and local profile while disabling the unfinished feature.
    pub fn enforce_wake_availability(&mut self) -> bool {
        let changed = !WAKE_WORD_AVAILABLE && self.wake_word_enabled;
        self.wake_word_enabled &= WAKE_WORD_AVAILABLE;
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
    fn previous_enabled_wake_is_disabled_without_losing_phrase_or_hotkey() {
        let mut settings: Settings = serde_json::from_value(serde_json::json!({
            "wake_word_enabled": true,
            "wake_word": "hey fono",
            "hotkey_mode": "toggle",
            "wake_word_threshold": 0.42
        }))
        .expect("previous wake settings deserialize");
        assert!(settings.enforce_wake_availability());
        assert!(!settings.wake_word_enabled);
        assert_eq!(settings.wake_word, "hey fono");
        assert_eq!(settings.hotkey_mode, HotkeyMode::Toggle);
        assert_eq!(settings.wake_word_threshold, 0.42);
        assert!(!settings.enforce_wake_availability());
    }

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

    #[test]
    fn pretranscription_is_opt_in_and_survives_settings_roundtrip() {
        let legacy: Settings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!legacy.pretranscription_enabled);
        assert!(!Settings::default().pretranscription_enabled);
        let enabled = Settings {
            pretranscription_enabled: true,
            ..legacy
        };
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(enabled).unwrap()).unwrap();
        assert!(restored.pretranscription_enabled);
        assert_eq!(restored.dictation_mode, DictationMode::Standard);
    }
}
