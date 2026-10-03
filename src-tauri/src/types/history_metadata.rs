//! Actual recognition facts for each saved session, independent of analytics.
use super::{AccelerationMode, AiMode, DictationMode, Settings, Transcript};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationBackend {
    Cpu,
    Cuda,
    Vulkan,
}
impl DictationBackend {
    pub fn from_device(device: Option<&str>) -> Option<Self> {
        match device {
            Some("CPU") => Some(Self::Cpu),
            Some("CUDA") => Some(Self::Cuda),
            Some("Vulkan") => Some(Self::Vulkan),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationHistoryMetadata {
    pub schema_version: u16,
    #[serde(default)]
    pub recording_duration_ms: Option<u64>,
    /// Stop until final text is generated, before inserting/saving the result.
    #[serde(default)]
    pub generation_duration_ms: Option<u64>,
    /// Time measured inside the selected STT engine, excluding load/queue wait.
    #[serde(default)]
    pub recognition_duration_ms: Option<u64>,
    /// Includes interactive scheduler wait plus model initialization.
    #[serde(default)]
    pub model_load_duration_ms: Option<u64>,
    #[serde(default)]
    pub processing_duration_ms: Option<u64>,
    /// Populated from the returned Transcript, never from the desired setting.
    #[serde(default)]
    pub backend: Option<DictationBackend>,
    /// File name only. The user's path is never stored in session metadata.
    #[serde(default)]
    pub model: Option<String>,
    pub requested_acceleration: AccelerationMode,
    pub language: String,
    #[serde(default)]
    pub detected_language: Option<String>,
    pub dictation_mode: DictationMode,
    pub processing_mode: AiMode,
    pub dictionary_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DictationTimingMeasurements {
    pub recording_duration_ms: Option<u64>,
    pub generation_duration_ms: Option<u64>,
    pub model_load_duration_ms: Option<u64>,
    pub processing_duration_ms: Option<u64>,
}

impl DictationHistoryMetadata {
    pub fn from_result(
        settings: &Settings,
        transcript: &Transcript,
        timings: DictationTimingMeasurements,
    ) -> Self {
        let model = settings.whisper_model_path.as_deref().and_then(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        });
        Self {
            schema_version: 1,
            recording_duration_ms: timings
                .recording_duration_ms
                .or_else(|| seconds_to_ms(transcript.audio_secs)),
            generation_duration_ms: timings.generation_duration_ms,
            recognition_duration_ms: seconds_to_ms(transcript.transcribe_secs),
            model_load_duration_ms: timings.model_load_duration_ms,
            processing_duration_ms: timings.processing_duration_ms,
            backend: DictationBackend::from_device(transcript.device.as_deref()),
            model,
            requested_acceleration: settings.acceleration,
            language: settings.language.clone(),
            detected_language: transcript.detected_language.clone(),
            dictation_mode: settings.dictation_mode,
            processing_mode: settings.ai_mode,
            dictionary_enabled: settings.personal_dictionary_enabled,
        }
    }
}

fn seconds_to_ms(seconds: Option<f32>) -> Option<u64> {
    seconds
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| (f64::from(s) * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transcript() -> Transcript {
        Transcript {
            text: "PRIVATE TEXT".into(),
            detected_language: Some("ru".into()),
            transcribe_secs: Some(1.25),
            audio_secs: Some(9.5),
            device: Some("CPU".into()),
        }
    }
    #[test]
    fn actual_backend_survives_auto_gpu_fallback_and_changed_preferences() {
        let settings = Settings {
            acceleration: AccelerationMode::Cuda,
            ..Settings::default()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &transcript(), Default::default());
        assert_eq!(metadata.backend, Some(DictationBackend::Cpu));
        assert_eq!(metadata.requested_acceleration, AccelerationMode::Cuda);
    }
    #[test]
    fn recording_uses_untrimmed_capture_measurement_and_generation_is_independent() {
        let metadata = DictationHistoryMetadata::from_result(
            &Settings::default(),
            &transcript(),
            DictationTimingMeasurements {
                recording_duration_ms: Some(10_000),
                generation_duration_ms: Some(2400),
                model_load_duration_ms: Some(900),
                processing_duration_ms: Some(250),
            },
        );
        assert_eq!(metadata.recording_duration_ms, Some(10_000));
        assert_eq!(metadata.generation_duration_ms, Some(2400));
        assert_eq!(metadata.recognition_duration_ms, Some(1250));
    }
    #[test]
    fn metadata_has_no_transcript_path_prompt_device_name_or_dictionary() {
        let settings = Settings {
            whisper_model_path: Some("private-directory/ggml-small.bin".into()),
            clean_prompt: Some("PRIVATE PROMPT".into()),
            ..Settings::default()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &transcript(), Default::default());
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("ggml-small.bin"));
        assert!(!json.contains("private-directory"));
        assert!(!json.contains("PRIVATE"));
    }
    #[test]
    fn invalid_durations_and_unknown_actual_backend_are_missing_not_fabricated() {
        let mut output = transcript();
        output.audio_secs = Some(f32::NAN);
        output.transcribe_secs = Some(-1.0);
        output.device = Some("unreported".into());
        let metadata = DictationHistoryMetadata::from_result(
            &Settings::default(),
            &output,
            Default::default(),
        );
        assert!(metadata.recording_duration_ms.is_none());
        assert!(metadata.recognition_duration_ms.is_none());
        assert!(metadata.backend.is_none());
    }
    #[test]
    fn legacy_history_has_no_metadata_and_still_deserializes() {
        let entry: super::super::DictationHistoryEntry = serde_json::from_value(serde_json::json!({
            "id": "old", "text": "existing result", "created_at": "2026-10-04T10:15:30Z", "device": "Vulkan"
        })).unwrap();
        assert!(entry.metadata.is_none());
        assert_eq!(entry.device.as_deref(), Some("Vulkan"));
    }
    #[test]
    fn numeric_session_facts_survive_clearing_opt_in_analytics() {
        let metadata = DictationHistoryMetadata::from_result(
            &Settings::default(),
            &transcript(),
            Default::default(),
        );
        let mut entry: super::super::DictationHistoryEntry = serde_json::from_value(serde_json::json!({
            "id": "saved", "text": "saved result", "created_at": "2026-10-04T10:15:30Z", "device": "CPU",
            "metadata": metadata, "original_text": "private original", "analytics_included": true
        })).unwrap();
        entry.clear_analytics_data(super::super::DictationAnalysisStatus::Disabled);
        assert!(entry.original_text.is_none());
        assert_eq!(entry.metadata.unwrap().recognition_duration_ms, Some(1250));
    }
}
