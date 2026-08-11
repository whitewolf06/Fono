//! Versioned backend events with legacy UI compatibility emissions.

use serde::Serialize;
use tauri::{Emitter, Runtime};

use crate::operation::OperationEvent;
use crate::stt::SttReadiness;
use crate::types::{PipelineState, Settings};

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
    PipelineState(PipelineStateEventV1),
    PipelineMode(PipelineModeV1),
    Wake(WakeEventV1),
    Settings(Box<SettingsEventV1>),
    SttReadiness(SttReadinessEventV1),
    ModelDownload(ModelDownloadEventV1),
    Error(ErrorEventV1),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PipelineStateEventV1 {
    pub state: PipelineState,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineModeV1 {
    Dictation,
    Command,
}

impl PipelineModeV1 {
    fn legacy(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Command => "command",
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadPhaseV1 {
    Started,
    Downloading,
    Verifying,
    Extracting,
    Completed,
    Cancelled,
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
    emit_v1(handle, BackendEventV1::Operation(event.clone()));
    let _ = handle.emit("operation-state", event);
}

pub fn emit_pipeline_state<R: Runtime>(handle: &tauri::AppHandle<R>, state: PipelineState) {
    emit_v1(
        handle,
        BackendEventV1::PipelineState(PipelineStateEventV1 { state }),
    );
    let _ = handle.emit("pipeline-state", state);
}

pub fn emit_pipeline_mode<R: Runtime>(handle: &tauri::AppHandle<R>, mode: PipelineModeV1) {
    emit_v1(handle, BackendEventV1::PipelineMode(mode));
    let _ = handle.emit("pipeline-mode", mode.legacy());
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
    let _ = handle.emit("wake-word-detected", phrase);
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
        BackendEventV1::SttReadiness(SttReadinessEventV1 {
            readiness: readiness.clone(),
        }),
    );
    let _ = handle.emit("stt-readiness", readiness);
}

pub fn emit_model_download<R: Runtime>(handle: &tauri::AppHandle<R>, event: ModelDownloadEventV1) {
    emit_v1(handle, BackendEventV1::ModelDownload(event.clone()));
    if event.phase == ModelDownloadPhaseV1::Completed {
        if event.download_id == "kws" {
            let _ = handle.emit("kws-model-downloaded", true);
        } else {
            let _ = handle.emit("model-downloaded", event.model);
        }
    }
}

pub fn emit_error<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    code: ErrorCodeV1,
    message: impl Into<String>,
    operation_id: Option<u64>,
) {
    let message = message.into();
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
