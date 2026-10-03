//! Closed fields form the redaction boundary: arbitrary text cannot enter a report.
use crate::{
    operation::OperationSource,
    types::{
        AccelerationMode, AiMode, InjectionMode, LlmConnectionKind, PipelineState, Settings,
        WhisperModelSize,
    },
};
use fono_wake::{WakeWordBackend, WakeWordStatus};

pub(super) struct ReportSnapshot {
    pub version: String,
    pub revision: String,
    pub profile: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub phase: PipelineState,
    pub paused: bool,
    pub model: &'static str,
    pub model_available: bool,
    pub language: &'static str,
    pub acceleration: AccelerationMode,
    pub active_backend: &'static str,
    pub stt_state: &'static str,
    pub cuda_available: bool,
    pub vulkan_available: bool,
    pub microphone_default: bool,
    pub microphone_available: Option<bool>,
    pub input_count: Option<usize>,
    pub capture_dropped: u64,
    pub subscriber_dropped: u64,
    pub audio_error: bool,
    pub wake_enabled: bool,
    pub wake_backend: WakeWordBackend,
    pub wake_status: WakeWordStatus,
    pub wake_calibrated: bool,
    pub processing: AiMode,
    pub processing_connection: Option<LlmConnectionKind>,
    pub processing_model_configured: bool,
    pub dictionary_enabled: bool,
    pub dictionary_count: usize,
    pub injection: InjectionMode,
    pub history_enabled: bool,
    pub trainer_enabled: bool,
    pub verbose_logging: bool,
    pub service_enabled: bool,
    pub service_error: bool,
    pub queue: [usize; 6],
}

pub(super) fn language_label(language: &str) -> &'static str {
    match language {
        "auto" => "авто",
        "ru" => "русский",
        "en" => "английский",
        _ => "другой язык",
    }
}

pub(super) fn model_label(settings: &Settings) -> &'static str {
    let Some(path) = settings.whisper_model_path.as_deref() else {
        return "не выбрана";
    };
    // Only public catalog identifiers are disclosed; custom file names are not.
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(WhisperModelSize::from_filename)
        .map(|model| model.api_identifier())
        .unwrap_or("пользовательская модель")
}
pub(super) fn backend_label(device: &str) -> &'static str {
    match device {
        "CUDA" => "CUDA",
        "Vulkan" => "Vulkan",
        "CPU" => "CPU",
        "Metal" => "Metal",
        _ => "неизвестно",
    }
}
pub(super) fn safe_version(value: &str) -> String {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 10 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        value.to_owned()
    } else {
        "unknown".into()
    }
}
pub(super) fn safe_revision(value: &str) -> String {
    if (7..=40).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        value.to_ascii_lowercase()
    } else {
        "unknown".into()
    }
}
pub(super) fn source_label(source: OperationSource) -> &'static str {
    match source {
        OperationSource::Ui => "главная страница",
        OperationSource::Hotkey => "горячая клавиша",
        OperationSource::WakeWord => "пробуждение",
        OperationSource::Diagnostics => "диагностика",
        OperationSource::Service => "API",
    }
}
