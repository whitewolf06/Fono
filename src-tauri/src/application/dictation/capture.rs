//! Capture setup, safety limit and leading-only VAD for ordinary dictation.
use super::{
    lifecycle::{emit_pipeline_error, Session},
    stop_with_reason_for,
};
use crate::{
    application::dictation_tail_diagnostics::DictationTailDiagnostic,
    error::{AppError, AppResult},
    operation::{OperationSource, TerminalReason},
    pipeline::{self, Pipeline},
    state::AppState,
    types::PipelineState,
};
use tauri::{AppHandle, Manager};

pub(crate) fn start(app: AppHandle, source: OperationSource) -> AppResult<()> {
    super::start_with_audio(app, source, &[], None)
}

pub(crate) fn start_with_audio(
    app: AppHandle,
    source: OperationSource,
    pre_roll: &[i16],
    cursor: Option<(u64, u64)>,
) -> AppResult<()> {
    start_with_audio_operation(app, source, pre_roll, cursor).map(|_| ())
}

pub(crate) fn start_operation(app: AppHandle, source: OperationSource) -> AppResult<u64> {
    start_with_audio_operation(app, source, &[], None)
}

pub(crate) fn start_with_audio_operation(
    app: AppHandle,
    source: OperationSource,
    pre_roll: &[i16],
    cursor: Option<(u64, u64)>,
) -> AppResult<u64> {
    start_configured(app, source, pre_roll, cursor, false)
}

#[allow(dead_code)] // Compatibility entry point; hotkeys need the operation-returning variant.
pub(crate) fn start_command(app: AppHandle) -> AppResult<()> {
    start_command_operation(app).map(|_| ())
}

pub(crate) fn start_command_operation(app: AppHandle) -> AppResult<u64> {
    start_configured(app, OperationSource::Hotkey, &[], None, true)
}

pub(crate) fn ensure_capture_allowed(app: &AppHandle) -> AppResult<()> {
    if crate::application::live_dictation::is_active(app) {
        return Err(AppError::Busy("Живая диктовка ещё завершается".into()));
    }
    if app.state::<AppState>().is_dictation_paused() {
        let message = "Запись приостановлена (через меню/траи).";
        emit_pipeline_error(app, message);
        return Err(AppError::Config(message.into()));
    }
    Ok(())
}

fn start_configured(
    app: AppHandle,
    source: OperationSource,
    pre_roll: &[i16],
    cursor: Option<(u64, u64)>,
    force_standard: bool,
) -> AppResult<u64> {
    let admission = crate::application::capture_configuration::begin_capture()?;
    ensure_capture_allowed(&app)?;
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();
    if !force_standard
        && settings.dictation_mode == crate::types::DictationMode::Live
        && source != OperationSource::Diagnostics
    {
        return crate::application::live_dictation::start(app.clone(), source, pre_roll, cursor);
    }
    let operation = pipeline
        .start_capture(
            settings.audio_device_id.as_deref(),
            pre_roll,
            source,
            cursor,
            false,
        )
        .map_err(|error| {
            emit_pipeline_error(&app, &error.to_string());
            tracing::error!(%error, "start_dictation: capture failed");
            error
        })?;
    if !pipeline.set_session_settings_for(operation, settings) {
        pipeline.stop_recording_for(operation)?;
        return Err(AppError::Cancelled("Диктовка отменена".into()));
    }
    drop(admission);
    super::workflow::capture_target(&app, operation, source);
    // The focus probe above deliberately runs outside all admission/capture
    // locks. Cancel may finish this operation while its provider is responding.
    // An old start must not pause WakeWord after idle cleanup already resumed it.
    if !super::capture_activation::activate(
        || {
            pipeline
                .while_operation(operation, || {
                    app.state::<fono_wake::WakeWordHandle>().pause();
                })
                .is_some()
        },
        || {
            pipeline::set_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Listening,
                TerminalReason::Completed,
            )
        },
        || super::resume_wake_if_idle(&app),
    ) {
        return Err(AppError::Cancelled("Диктовка отменена".into()));
    }
    arm_recording_safety_timeout(app.clone(), operation, force_standard);
    Ok(operation)
}

fn arm_recording_safety_timeout(app: AppHandle, operation: u64, command: bool) {
    tauri::async_runtime::spawn(async move {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(pipeline::MAX_RECORDING_SECONDS as u64);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::time::sleep(remaining.min(std::time::Duration::from_secs(1))).await;
            let pipeline = app.state::<Pipeline>();
            if !pipeline.is_operation_active(operation) || !pipeline.is_recording() {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
        tracing::warn!(
            operation,
            max_seconds = pipeline::MAX_RECORDING_SECONDS,
            "recording safety timeout reached"
        );
        emit_pipeline_error(
            &app,
            &format!(
                "Достигнут предел {} секунд. Сохранённый звук будет распознан.",
                pipeline::MAX_RECORDING_SECONDS
            ),
        );
        if command {
            if let Err(error) = crate::run_voice_command(&app, operation).await {
                emit_pipeline_error(&app, &error.to_string());
            }
        } else if let Err(error) = stop_with_reason_for(
            app.clone(),
            operation,
            crate::application::dictation_tail_diagnostics::DictationStopReason::Manual,
        )
        .await
        {
            emit_pipeline_error(&app, &error.to_string());
        }
    });
}

pub(super) fn finish_samples(
    session: &Session,
    diagnostic: &mut DictationTailDiagnostic,
) -> AppResult<Option<Vec<i16>>> {
    let pipeline = session.app.state::<Pipeline>();
    // Stop and drain the audio subscriber before loading a model.
    let samples = pipeline
        .stop_recording_for(session.operation)
        .map_err(|error| {
            diagnostic.emit();
            session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
            error
        })?;
    let Some(samples) = samples else {
        diagnostic.stt_cancelled();
        return Ok(None);
    };
    diagnostic.record_capture(samples.len());
    if pipeline.recording_limit_reached() {
        emit_pipeline_error(&session.app, &format!("Recording reached the {} second safety limit; the captured part will be transcribed.", pipeline::MAX_RECORDING_SECONDS));
    }
    if !session.active("after recording") {
        diagnostic.stt_cancelled();
        return Ok(None);
    }
    if samples.is_empty() {
        return Ok(None);
    }
    let mut speech_gate = crate::application::speech_gate::SpeechGate::new(&session.app)?;
    if !speech_gate.recording_has_speech(&samples, || session.cancellation.is_cancelled()) {
        tracing::debug!(
            operation = session.operation,
            captured_samples = samples.len(),
            "dictation skipped: no confirmed speech in capture"
        );
        return Ok(None);
    }
    if !session.active("after speech validation") {
        diagnostic.stt_cancelled();
        return Ok(None);
    }
    let captured_samples = samples.len();
    // Only leading silence may be removed. Wake handoff keeps its exact boundary.
    let (trimmed, vad) = crate::vad::trim_leading_silence_with_result(&samples);
    let samples = if session.source == OperationSource::WakeWord {
        diagnostic.skip_vad(captured_samples);
        samples
    } else {
        let retained_trailing = captured_samples.saturating_sub(vad.speech_end_sample);
        diagnostic.record_vad(captured_samples, trimmed.len(), &vad, retained_trailing);
        trimmed
    };
    if samples.is_empty() {
        return Ok(None);
    }
    if !session.transition(PipelineState::Transcribing, TerminalReason::Completed) {
        diagnostic.stt_cancelled();
        return Ok(None);
    }
    Ok(Some(samples))
}
