//! Actual session facts, plus a separately removable opt-in VAD summary.
use super::{
    AccelerationMode, AiMode, DictationMode, ProcessingWorkflow, Settings, TextPreset, Transcript,
    TranslationLanguage,
};
use fono_types::RemoteBackend;
use serde::{Deserialize, Deserializer, Serialize};

const MAX_DURATION_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationBackend {
    Cpu,
    Cuda,
    Vulkan,
    Remote,
    /// Explicit future/invalid serialized values are not legacy missing data.
    Unknown,
}
impl DictationBackend {
    pub fn from_device(device: Option<&str>) -> Option<Self> {
        match device {
            Some("CPU") => Some(Self::Cpu),
            Some("CUDA") => Some(Self::Cuda),
            Some("Vulkan") => Some(Self::Vulkan),
            Some(device) if remote_device(device).is_some() => Some(Self::Remote),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationHistoryMetadata {
    pub schema_version: u16,
    /// True only when this session actually succeeded using the local reserve.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fallback: bool,
    /// Separate opt-in from speech analytics. Never transmitted to an LLM/API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_context: Option<super::ApplicationContext>,
    /// Opt-in VAD aggregate, unlike the independent technical session facts below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_timing: Option<super::SpeechTimingSummary>,
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
    #[serde(default, deserialize_with = "deserialize_backend")]
    pub backend: Option<DictationBackend>,
    /// User-provided label from the frozen session. Never inferred from its URL.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_remote_name"
    )]
    pub remote_name: Option<String>,
    /// Protocol device code, not a hardware name or a profile address.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_remote_device"
    )]
    pub remote_device: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_remote_backend"
    )]
    pub remote_backend: Option<RemoteBackend>,
    /// End-to-end client request duration; not network-only time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_duration"
    )]
    pub request_duration_ms: Option<u64>,
    /// Server total can include queueing, loading and inference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_duration"
    )]
    pub server_total_duration_ms: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_duration"
    )]
    pub network_duration_ms: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_duration"
    )]
    pub decode_duration_ms: Option<u64>,
    #[serde(default)]
    pub recognizer: Option<super::SttEngine>,
    /// Local file name or remote public model ID, never the user's path or URL.
    #[serde(default)]
    pub model: Option<String>,
    pub requested_acceleration: AccelerationMode,
    pub language: String,
    #[serde(default)]
    pub detected_language: Option<String>,
    pub dictation_mode: DictationMode,
    pub processing_mode: AiMode,
    #[serde(default)]
    pub processing_workflow: Option<ProcessingWorkflow>,
    #[serde(default)]
    pub processing_preset: Option<TextPreset>,
    #[serde(default)]
    pub processing_target_language: Option<TranslationLanguage>,
    pub dictionary_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DictationTimingMeasurements {
    pub speech_timing: Option<super::SpeechTimingSummary>,
    pub recording_duration_ms: Option<u64>,
    pub generation_duration_ms: Option<u64>,
    pub model_load_duration_ms: Option<u64>,
    pub processing_duration_ms: Option<u64>,
}

impl DictationHistoryMetadata {
    /// Loading old/future JSON must keep the result while rejecting impossible
    /// relationships between independently optional technical measurements.
    pub(crate) fn normalize_remote_facts(&mut self) {
        if self.backend != Some(DictationBackend::Remote) {
            return;
        }
        if self
            .remote_backend
            .zip(self.remote_device.as_deref())
            .is_some_and(|(backend, device)| !backend.valid_device(device))
        {
            self.remote_backend = None;
            self.remote_device = None;
        }
        if self
            .decode_duration_ms
            .zip(self.server_total_duration_ms)
            .is_some_and(|(decode, total)| decode > total)
        {
            self.decode_duration_ms = None;
            self.recognition_duration_ms = None;
        }
        if self
            .network_duration_ms
            .zip(self.request_duration_ms)
            .is_some_and(|(network, request)| network > request)
        {
            self.network_duration_ms = None;
        }
    }

    pub fn from_result(
        settings: &Settings,
        transcript: &Transcript,
        timings: DictationTimingMeasurements,
    ) -> Self {
        let selection = settings.selected_stt_model();
        let remote = transcript.remote.as_ref().filter(|remote| remote.valid());
        let coded_device = remote
            .map(|remote| remote.device.as_str())
            .or_else(|| transcript.device.as_deref().and_then(remote_device));
        let is_remote = coded_device.is_some();
        let active_model = remote.map(|remote| remote.model.clone()).or_else(|| {
            if is_remote {
                settings
                    .remote_stt
                    .as_ref()
                    .map(|remote| remote.model.clone())
            } else {
                selection.as_ref().map(|model| model.id.clone())
            }
        });
        let recognizer = active_model.as_ref().map(|model| model.engine);
        let model = if is_remote {
            active_model.map(|model| match model.engine {
                super::SttEngine::Whisper => format!("whisper:{}", model.variant),
                _ => model.public_id(),
            })
        } else {
            selection.as_ref().map(|m| {
                if m.id.engine == super::SttEngine::Whisper {
                    m.local_path
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("")
                        .to_owned()
                } else {
                    m.id.public_id()
                }
            })
        };
        Self {
            schema_version: 1,
            fallback: settings.stt_fallback && !is_remote,
            application_context: None,
            speech_timing: (settings.analytics_enabled && settings.speech_trainer_enabled)
                .then_some(timings.speech_timing)
                .flatten()
                .filter(|value| value.valid()),
            recording_duration_ms: timings
                .recording_duration_ms
                .or_else(|| seconds_to_ms(transcript.audio_secs)),
            generation_duration_ms: timings.generation_duration_ms,
            recognition_duration_ms: if is_remote {
                remote.and_then(|remote| remote.decode_ms)
            } else {
                seconds_to_ms(transcript.transcribe_secs)
            },
            model_load_duration_ms: timings.model_load_duration_ms,
            processing_duration_ms: timings.processing_duration_ms,
            backend: if is_remote {
                Some(DictationBackend::Remote)
            } else {
                DictationBackend::from_device(transcript.device.as_deref())
            },
            remote_name: is_remote
                .then(|| {
                    settings
                        .remote_stt
                        .as_ref()?
                        .display_name
                        .as_deref()
                        .and_then(safe_remote_name)
                })
                .flatten(),
            remote_device: coded_device.map(str::to_owned),
            remote_backend: remote
                .map(|remote| remote.backend)
                .or_else(|| coded_device.and_then(physical_backend)),
            request_duration_ms: remote.map(|remote| remote.request_ms),
            server_total_duration_ms: remote.map(|remote| remote.server_total_ms),
            network_duration_ms: remote.and_then(|remote| remote.network_ms),
            decode_duration_ms: remote.and_then(|remote| remote.decode_ms),
            recognizer,
            model,
            requested_acceleration: settings.acceleration,
            language: settings.language.clone(),
            detected_language: transcript.detected_language.clone(),
            dictation_mode: settings.dictation_mode,
            processing_mode: settings.ai_mode,
            processing_workflow: Some(settings.processing_workflow),
            processing_preset: (settings.ai_mode != AiMode::Off)
                .then(|| crate::application::dictation::workflow::effective_preset(settings)),
            processing_target_language: (settings.ai_mode != AiMode::Off)
                .then(|| crate::application::dictation::workflow::effective_language(settings))
                .flatten(),
            dictionary_enabled: settings.personal_dictionary_enabled,
        }
    }
}

fn remote_device(value: &str) -> Option<&str> {
    value
        .strip_prefix("remote:")
        .filter(|device| physical_backend(device).is_some())
}

fn physical_backend(device: &str) -> Option<RemoteBackend> {
    match device {
        "cpu" => Some(RemoteBackend::Cpu),
        "vulkan" => Some(RemoteBackend::Vulkan),
        "cuda" => Some(RemoteBackend::Cuda),
        device
            if device.strip_prefix("cuda:").is_some_and(|index| {
                !index.is_empty()
                    && index.len() <= 3
                    && index.bytes().all(|byte| byte.is_ascii_digit())
            }) =>
        {
            Some(RemoteBackend::Cuda)
        }
        _ => None,
    }
}

fn safe_remote_name(value: &str) -> Option<String> {
    let name = value.trim();
    (!name.is_empty()
        && name.chars().count() <= 80
        && !name.chars().any(char::is_control)
        && !name.contains("://")
        && !name.contains(['/', '\\', '@']))
    .then(|| name.to_owned())
}

fn deserialize_backend<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DictationBackend>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(|value| match value.as_str() {
        Some("cpu") => DictationBackend::Cpu,
        Some("cuda") => DictationBackend::Cuda,
        Some("vulkan") => DictationBackend::Vulkan,
        Some("remote") => DictationBackend::Remote,
        _ => DictationBackend::Unknown,
    }))
}

fn deserialize_remote_backend<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RemoteBackend>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(|value| match value {
            "cpu" => Some(RemoteBackend::Cpu),
            "cuda" => Some(RemoteBackend::Cuda),
            "vulkan" => Some(RemoteBackend::Vulkan),
            _ => None,
        }))
}

fn deserialize_remote_name<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(safe_remote_name))
}

fn deserialize_remote_device<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .filter(|device| physical_backend(device).is_some())
        .map(str::to_owned))
}

fn deserialize_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value <= MAX_DURATION_MS))
}

fn seconds_to_ms(seconds: Option<f32>) -> Option<u64> {
    seconds
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| (f64::from(s) * 1000.0).round() as u64)
        .filter(|duration| *duration <= MAX_DURATION_MS)
}

#[cfg(test)]
#[path = "history_metadata/remote_tests.rs"]
mod remote_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_is_an_actual_session_fact_and_old_history_defaults_to_false() {
        let settings = Settings {
            stt_fallback: true,
            acceleration: AccelerationMode::Cpu,
            whisper_model_path: Some("PRIVATE/ggml-tiny.bin".into()),
            ..Settings::default()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &transcript(), Default::default());
        assert!(metadata.fallback);
        assert_eq!(metadata.model.as_deref(), Some("ggml-tiny.bin"));
        assert_eq!(metadata.requested_acceleration, AccelerationMode::Cpu);
        let mut json = serde_json::to_value(metadata).unwrap();
        json.as_object_mut().unwrap().remove("fallback");
        let legacy: DictationHistoryMetadata = serde_json::from_value(json).unwrap();
        assert!(!legacy.fallback);
    }
    #[test]
    fn remote_history_uses_the_session_remote_model_without_exposing_dormant_local_paths_or_endpoint(
    ) {
        let settings = Settings {
            whisper_model_path: Some("PRIVATE_DIRECTORY/ggml-small.bin".into()),
            stt_source: super::super::SttSource::FonoServer,
            remote_stt: Some(super::super::RemoteSttSettings {
                url: "https://PRIVATE_SERVER.invalid".into(),
                display_name: None,
                model: super::super::ModelId {
                    engine: super::super::SttEngine::Whisper,
                    variant: "large_turbo".into(),
                },
                ca_pem: None,
                ssh: None,
                fallback: Default::default(),
            }),
            ..Settings::default()
        };
        let result = Transcript {
            device: Some("remote:cuda:0".into()),
            ..transcript()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &result, Default::default());
        assert_eq!(metadata.recognizer, Some(super::super::SttEngine::Whisper));
        assert_eq!(metadata.model.as_deref(), Some("whisper:large_turbo"));
        assert_eq!(metadata.backend, Some(DictationBackend::Remote));
        assert_eq!(metadata.remote_device.as_deref(), Some("cuda:0"));
        assert!(metadata.recognition_duration_ms.is_none());
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(!json.contains("PRIVATE"));
        assert!(!json.contains("ggml-small.bin"));
    }
    #[cfg(feature = "offline-stt")]
    #[test]
    fn offline_metadata_uses_session_identity_and_actual_device_without_package_path() {
        let spec = crate::stt::offline_models::specifications()
            .unwrap()
            .remove(0);
        let settings = Settings {
            stt_model: Some(super::super::SttModelSelection {
                id: spec.id.clone(),
                local_path: "PRIVATE/package".into(),
            }),
            ..Settings::default()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &transcript(), Default::default());
        assert_eq!(metadata.recognizer, Some(spec.id.engine));
        assert_eq!(metadata.model, Some(spec.id.public_id()));
        assert_eq!(metadata.backend, Some(DictationBackend::Cpu));
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("PRIVATE"));
    }
    #[test]
    fn disabled_translation_does_not_record_the_remembered_language_as_applied() {
        let settings = Settings {
            ai_mode: AiMode::Clean,
            processing_translation_enabled: false,
            processing_target_language: Some(TranslationLanguage::En),
            ..Settings::default()
        };
        let metadata = DictationHistoryMetadata::from_result(
            &settings,
            &transcript(),
            DictationTimingMeasurements::default(),
        );
        assert_eq!(metadata.processing_target_language, None);
        assert_eq!(
            settings.processing_target_language,
            Some(TranslationLanguage::En)
        );
    }
    fn transcript() -> Transcript {
        Transcript {
            text: "PRIVATE TEXT".into(),
            detected_language: Some("ru".into()),
            transcribe_secs: Some(1.25),
            audio_secs: Some(9.5),
            device: Some("CPU".into()),
            remote: None,
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
                speech_timing: None,
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
    #[test]
    fn applied_workflow_preset_and_translation_remain_session_facts() {
        let settings = Settings {
            processing_workflow: ProcessingWorkflow::Manual,
            processing_preset: Some(TextPreset::Task),
            processing_target_language: Some(TranslationLanguage::En),
            ai_mode: AiMode::Clean,
            ..Settings::default()
        };
        let metadata =
            DictationHistoryMetadata::from_result(&settings, &transcript(), Default::default());
        assert_eq!(
            metadata.processing_workflow,
            Some(ProcessingWorkflow::Manual)
        );
        assert_eq!(metadata.processing_preset, Some(TextPreset::Task));
        assert_eq!(
            metadata.processing_target_language,
            Some(TranslationLanguage::En)
        );
        let disabled = Settings {
            ai_mode: AiMode::Off,
            ..settings
        };
        let raw =
            DictationHistoryMetadata::from_result(&disabled, &transcript(), Default::default());
        assert_eq!(raw.processing_preset, None);
        assert_eq!(raw.processing_target_language, None);
    }
    #[test]
    fn legacy_metadata_without_workflow_fields_still_deserializes() {
        let metadata = DictationHistoryMetadata::from_result(
            &Settings::default(),
            &transcript(),
            Default::default(),
        );
        let mut json = serde_json::to_value(metadata).unwrap();
        for field in [
            "processing_workflow",
            "processing_preset",
            "processing_target_language",
        ] {
            json.as_object_mut().unwrap().remove(field);
        }
        let legacy: DictationHistoryMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.processing_workflow, None);
        assert_eq!(legacy.processing_preset, None);
        assert_eq!(legacy.processing_target_language, None);
    }
}
