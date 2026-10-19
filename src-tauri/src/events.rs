//! Versioned backend events and channels consumed by the current UI.

use serde::Serialize;
use tauri::{Emitter, Manager, Runtime};

use crate::stt::SttReadiness;
use crate::types::{PipelineState, Settings};
use fono_core::OperationEvent;

pub const EVENT_CHANNEL_V1: &str = "backend-event-v1";
const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelopeV1<T> {
    pub schema_version: u16,
    #[serde(flatten)]
    pub event: T,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum BackendEventV1 {
    Operation(OperationEvent),
    DictationOutcome(DictationOutcomeEventV1),
    PipelineState(PipelineStateEventV1),
    PipelineMode(PipelineModeV1),
    Wake(WakeEventV1),
    Settings(Box<SettingsEventV1>),
    SttReadiness(SttReadinessEventV1),
    ModelDownload(ModelDownloadEventV1),
    Service(ServiceEventV1),
    HistoryChanged(()),
    GpuMemoryChanged(crate::application::gpu_memory::SttMemoryStatus),
    AudioLevel(AudioLevelEventV1),
    Error(ErrorEventV1),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PipelineStateEventV1 {
    pub state: PipelineState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DictationOutcomeEventV1 {
    pub operation_id: u64,
    pub outcome: crate::application::dictation::DictationOutcomeKind,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineModeV1 {
    Dictation,
    Command,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WakeEventV1 {
    Status { status: WakeStatusV1 },
    Detected { phrase: String },
    Countdown(WakeCountdownV1),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WakeCountdownV1 {
    pub remaining_ms: u64,
    pub timeout_ms: u64,
    pub speaking: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeStatusV1 {
    Listening,
    Paused,
    Loading,
    MissingModel,
}

impl WakeStatusV1 {
    fn legacy(self) -> &'static str {
        match self {
            Self::Listening => "listening",
            Self::Paused => "paused",
            Self::Loading => "loading",
            Self::MissingModel => "missing_model",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsEventV1 {
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize)]
pub struct SttReadinessEventV1 {
    pub readiness: SttReadiness,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadEventV1 {
    pub download_id: String,
    pub model: String,
    pub phase: ModelDownloadPhaseV1,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ServiceEventV1;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AudioLevelEventV1 {
    pub operation_id: u64,
    pub level: f32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadPhaseV1 {
    Started,
    Downloading,
    Verifying,
    Extracting,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEventV1 {
    pub code: ErrorCodeV1,
    pub message: String,
    pub operation_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCodeV1 {
    Audio,
    Stt,
    Llm,
    Injection,
    Download,
    Settings,
    Wake,
    Internal,
}

fn emit_v1<R: Runtime>(handle: &tauri::AppHandle<R>, event: BackendEventV1) {
    let _ = handle.emit(
        EVENT_CHANNEL_V1,
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event,
        },
    );
}

pub fn emit_operation<R: Runtime>(handle: &tauri::AppHandle<R>, event: OperationEvent) {
    emit_v1(handle, BackendEventV1::Operation(event));
}

pub(crate) fn emit_dictation_outcome<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    operation_id: u64,
    outcome: crate::application::dictation::DictationOutcomeKind,
) {
    let event = DictationOutcomeEventV1 {
        operation_id,
        outcome,
    };
    if let Some(session) = handle.try_state::<crate::ipc::desktop_v3::DesktopSession>() {
        if !session.record_outcome(event) {
            return;
        }
    }
    emit_v1(handle, BackendEventV1::DictationOutcome(event));
}

pub fn emit_pipeline_state<R: Runtime>(handle: &tauri::AppHandle<R>, state: PipelineState) {
    emit_v1(
        handle,
        BackendEventV1::PipelineState(PipelineStateEventV1 { state }),
    );
}

pub fn emit_pipeline_mode<R: Runtime>(handle: &tauri::AppHandle<R>, mode: PipelineModeV1) {
    emit_v1(handle, BackendEventV1::PipelineMode(mode));
}

pub fn emit_wake_status<R: Runtime>(handle: &tauri::AppHandle<R>, status: WakeStatusV1) {
    emit_v1(handle, BackendEventV1::Wake(WakeEventV1::Status { status }));
    let _ = handle.emit("wake-word-status", status.legacy());
}

pub fn emit_wake_detected<R: Runtime>(handle: &tauri::AppHandle<R>, phrase: &str) {
    emit_v1(
        handle,
        BackendEventV1::Wake(WakeEventV1::Detected {
            phrase: phrase.to_string(),
        }),
    );
}

pub fn emit_wake_countdown<R: Runtime>(handle: &tauri::AppHandle<R>, countdown: WakeCountdownV1) {
    emit_v1(
        handle,
        BackendEventV1::Wake(WakeEventV1::Countdown(countdown)),
    );
    let _ = handle.emit("wake-dictation-countdown", countdown);
}

pub fn emit_settings<R: Runtime>(handle: &tauri::AppHandle<R>, settings: &Settings) {
    emit_v1(
        handle,
        BackendEventV1::Settings(Box::new(SettingsEventV1 {
            settings: settings.clone(),
        })),
    );
    let _ = handle.emit("settings-changed", settings.clone());
}

pub fn emit_stt_readiness<R: Runtime>(handle: &tauri::AppHandle<R>, readiness: SttReadiness) {
    emit_v1(
        handle,
        BackendEventV1::SttReadiness(SttReadinessEventV1 { readiness }),
    );
}

pub fn emit_model_download<R: Runtime>(handle: &tauri::AppHandle<R>, event: ModelDownloadEventV1) {
    emit_v1(handle, BackendEventV1::ModelDownload(event));
}

pub fn emit_service_changed<R: Runtime>(handle: &tauri::AppHandle<R>) {
    emit_v1(handle, BackendEventV1::Service(ServiceEventV1));
    let _ = handle.emit("local-transcription-service-changed", ());
}

pub fn emit_history_changed(handle: &tauri::AppHandle) {
    emit_v1(handle, BackendEventV1::HistoryChanged(()));
}

pub(crate) fn emit_gpu_memory_changed(
    handle: &tauri::AppHandle,
    status: crate::application::gpu_memory::SttMemoryStatus,
) {
    emit_v1(handle, BackendEventV1::GpuMemoryChanged(status));
}

pub fn emit_audio_level(handle: &tauri::AppHandle, event: AudioLevelEventV1) {
    emit_v1(handle, BackendEventV1::AudioLevel(event));
}

pub fn emit_error<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    code: ErrorCodeV1,
    message: impl Into<String>,
    operation_id: Option<u64>,
) {
    let message = crate::error::safe_user_message(
        match code {
            ErrorCodeV1::Audio => crate::error::UserErrorCode::Audio,
            ErrorCodeV1::Stt => crate::error::UserErrorCode::Stt,
            ErrorCodeV1::Llm => crate::error::UserErrorCode::Llm,
            ErrorCodeV1::Injection => crate::error::UserErrorCode::Injection,
            ErrorCodeV1::Download => crate::error::UserErrorCode::Download,
            ErrorCodeV1::Settings => crate::error::UserErrorCode::Settings,
            ErrorCodeV1::Wake => crate::error::UserErrorCode::Wake,
            ErrorCodeV1::Internal => crate::error::UserErrorCode::Internal,
        },
        &message.into(),
    );
    emit_v1(
        handle,
        BackendEventV1::Error(ErrorEventV1 {
            code,
            message: message.clone(),
            operation_id,
        }),
    );
    let _ = handle.emit("error", message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_event_contains_only_the_captured_operation_and_typed_status() {
        let encoded = serde_json::to_value(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event: BackendEventV1::DictationOutcome(DictationOutcomeEventV1 {
                operation_id: 17,
                outcome: crate::application::dictation::DictationOutcomeKind::NoSpeech,
            }),
        })
        .unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "schema_version": 1, "kind": "dictation_outcome",
                "payload": {"operation_id": 17, "outcome": "no_speech"}
            })
        );
    }

    #[test]
    fn event_envelope_is_versioned_and_typed() {
        let encoded = serde_json::to_value(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event: BackendEventV1::PipelineState(PipelineStateEventV1 {
                state: PipelineState::Listening,
            }),
        })
        .unwrap();

        assert_eq!(encoded["schema_version"], 1);
        assert_eq!(encoded["kind"], "pipeline_state");
        assert_eq!(encoded["payload"]["state"], "listening");
    }

    #[test]
    fn model_download_phases_are_stable_snake_case_values() {
        assert_eq!(
            serde_json::to_string(&ModelDownloadPhaseV1::Verifying).unwrap(),
            "\"verifying\""
        );
    }

    #[test]
    fn stt_readiness_event_is_versioned_and_typed() {
        let encoded = serde_json::to_value(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event: BackendEventV1::SttReadiness(SttReadinessEventV1 {
                readiness: SttReadiness::Loading,
            }),
        })
        .unwrap();

        assert_eq!(encoded["kind"], "stt_readiness");
        assert_eq!(encoded["payload"]["readiness"]["state"], "loading");
    }
}
