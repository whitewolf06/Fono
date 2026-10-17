//! Final dictation results and optional local speech analytics.

use super::{AiMode, DictationHistoryMetadata};
use serde::{Deserialize, Serialize};

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

fn default_history_analytics_included() -> bool {
    true
}

#[cfg(test)]
mod dictation_history_tests {
    use super::{DictationAnalysisStatus, DictationHistoryEntry};
    use crate::types::Settings;

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
    fn legacy_gpu_residency_stays_resident_and_adaptive_is_persisted() {
        use crate::types::GpuModelResidency;
        let legacy: Settings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(legacy.gpu_model_residency, GpuModelResidency::Resident);
        let adaptive = Settings {
            gpu_model_residency: GpuModelResidency::Adaptive,
            ..legacy
        };
        let json = serde_json::to_value(&adaptive).unwrap();
        assert_eq!(json["gpu_model_residency"], "adaptive");
        let roundtrip: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.gpu_model_residency, GpuModelResidency::Adaptive);
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
