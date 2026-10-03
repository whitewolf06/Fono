//! Explicit, local diagnostic export. Does not read logs, audio or history text.
mod model;
mod render;
pub mod telemetry;
#[cfg(test)]
mod tests;

use crate::{application::service_control::ServiceControl, pipeline::Pipeline, state::AppState};
use fono_wake::{AudioHub, WakeWordHandle};
use model::ReportSnapshot;
use tauri::{AppHandle, Manager};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    schema_version: u16,
    runtime: &'static str,
    text: String,
}

pub fn collect(app: &AppHandle) -> DiagnosticReport {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let pipeline = app.state::<Pipeline>();
    let stt = pipeline.stt();
    let (stt_state, active_backend) = match stt.readiness() {
        crate::stt::SttReadiness::Unloaded => ("не загружен", "нет"),
        crate::stt::SttReadiness::Loading => ("загрузка", "нет"),
        crate::stt::SttReadiness::Ready { device } => {
            if stt.is_loaded() {
                ("готов", model::backend_label(&device))
            } else {
                ("не загружен", "нет")
            }
        }
        crate::stt::SttReadiness::Failed { .. } => ("ошибка", "нет"),
    };
    let (cuda_available, vulkan_available) = crate::stt::worker_paths_for_app(app).capabilities();
    // Enumeration does not open a microphone or record a sample. Names are
    // compared transiently and are never copied into the report.
    let devices = crate::audio::AudioCapture::list_input_devices().ok();
    let microphone_available = devices.as_ref().map(|devices| {
        devices
            .iter()
            .any(|device| match settings.audio_device_id.as_deref() {
                None => device.is_default,
                Some(id) => id == device.id,
            })
    });
    let audio = app.state::<AudioHub>().diagnostics();
    let service = app.state::<ServiceControl>().snapshot();
    let queue = service.snapshot.queue;
    let snapshot = ReportSnapshot {
        version: model::safe_version(env!("CARGO_PKG_VERSION")),
        revision: model::safe_revision(env!("FONO_BUILD_REVISION")),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        phase: state.pipeline_state(),
        paused: state.is_dictation_paused(),
        model: model::model_label(&settings),
        model_available: settings
            .whisper_model_path
            .as_deref()
            .is_some_and(|p| std::path::Path::new(p).is_file()),
        language: model::language_label(&settings.language),
        acceleration: settings.acceleration,
        active_backend,
        stt_state,
        cuda_available,
        vulkan_available,
        microphone_default: settings.audio_device_id.is_none(),
        microphone_available,
        input_count: devices.as_ref().map(Vec::len),
        capture_dropped: audio.dropped_capture_packets,
        subscriber_dropped: audio.dropped_subscriber_packets,
        audio_error: audio.last_error.is_some(),
        wake_enabled: settings.wake_word_enabled,
        wake_backend: settings.wake_backend,
        wake_status: app.state::<WakeWordHandle>().status(),
        wake_calibrated: settings.wake_calibration_profile.is_some(),
        processing: settings.ai_mode,
        processing_connection: settings
            .correction_profile()
            .map(|profile| profile.connection),
        processing_model_configured: settings
            .text_correction_llm
            .model
            .as_ref()
            .is_some_and(|model| !model.is_empty())
            || settings
                .correction_profile()
                .and_then(|profile| profile.model.as_ref())
                .is_some_and(|model| !model.is_empty()),
        dictionary_enabled: settings.personal_dictionary_enabled,
        dictionary_count: settings.personal_dictionary_entries.len(),
        injection: settings.injection_mode,
        history_enabled: settings.history_enabled,
        trainer_enabled: settings.speech_trainer_enabled,
        verbose_logging: settings.verbose_logging,
        service_enabled: service.enabled,
        service_error: service.error.is_some(),
        queue: [
            queue.queued,
            queue.preparing,
            queue.transcribing,
            queue.completed,
            queue.failed,
            queue.cancelled,
        ],
    };
    DiagnosticReport {
        schema_version: 1,
        runtime: "native",
        text: render::render(&snapshot, telemetry::last_measurement().as_ref()),
    }
}
