//! Capture setup, safety limit and one neural speech decision for dictation.
use super::{
    lifecycle::{emit_pipeline_error, Session},
    stop_with_reason_for,
};
use crate::{
    application::dictation_tail_diagnostics::DictationTailDiagnostic,
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{PipelineState, Settings},
};
use fono_core::{OperationSource, TerminalReason};
use tauri::{AppHandle, Manager};

/// Capture drain, neural model I/O and offline VAD belong to an admitted worker.
/// Dropping the IPC waiter cannot release update admission before this work ends.
pub(super) async fn finish_samples_async(
    session: Session,
    mut diagnostic: DictationTailDiagnostic,
    report: std::sync::Arc<crate::application::diagnostic_report::telemetry::ReportTrace>,
) -> AppResult<(CapturedSamples, DictationTailDiagnostic)> {
    let activity = crate::application::updates::activity::lease()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _timer = report
            .stage(crate::application::diagnostic_report::telemetry::ReportStage::CaptureFinish);
        let samples = finish_samples(&session, &mut diagnostic, &report)?;
        Ok((samples, diagnostic))
    })
    .await
    .map_err(|_| AppError::Internal("capture completion task failed".into()))?
}

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

pub(crate) fn start_command_operation(app: AppHandle) -> AppResult<u64> {
    start_configured(app, OperationSource::Hotkey, &[], None, true)
}

pub(crate) struct ReservedCapture {
    pub operation: u64,
    settings: Settings,
}

/// Memory-only CLI admission; the operation and snapshot exist before queueing.
pub(crate) fn reserve_operation(app: &AppHandle) -> AppResult<ReservedCapture> {
    let _admission = crate::application::capture_configuration::try_begin_capture()?;
    ensure_capture_allowed(app)?;
    let pipeline = app.state::<Pipeline>();
    let settings = app.state::<AppState>().settings();
    let operation = pipeline.reserve_capture(OperationSource::Hotkey)?;
    if !pipeline.set_session_settings_for(operation, settings.clone()) {
        return Err(AppError::Cancelled("Диктовка отменена".into()));
    }
    Ok(ReservedCapture {
        operation,
        settings,
    })
}

pub(crate) fn start_reserved_operation(app: AppHandle, capture: ReservedCapture) -> AppResult<u64> {
    let admission = crate::application::capture_configuration::begin_capture()?;
    ensure_capture_allowed(&app)?;
    app.state::<Pipeline>().start_reserved_capture(
        capture.operation,
        capture.settings.audio_device_id.as_deref(),
    )?;
    configure_recording(&app, capture.operation, &capture.settings, false)?;
    drop(admission);
    activate_recording(app, capture.operation, OperationSource::Hotkey, false)
}

pub(crate) fn ensure_capture_allowed(app: &AppHandle) -> AppResult<()> {
    #[cfg(feature = "experimental-live")]
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
    let pipeline = app.state::<Pipeline>();
    let settings = app.state::<AppState>().settings();
    #[cfg(feature = "experimental-live")]
    if !force_standard
        && settings.dictation_mode == crate::types::DictationMode::Live
        && source != OperationSource::Diagnostics
    {
        return crate::application::live_dictation::start(app.clone(), source, pre_roll, cursor);
    }
    #[cfg(not(feature = "experimental-live"))]
    let _ = force_standard;
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
    configure_recording(&app, operation, &settings, force_standard)?;
    drop(admission);
    activate_recording(app.clone(), operation, source, force_standard)
}

fn configure_recording(
    app: &AppHandle,
    operation: u64,
    settings: &Settings,
    force_standard: bool,
) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    pipeline.set_command_capture(operation, force_standard);
    if !pipeline.set_session_settings_for(operation, settings.clone()) {
        pipeline.stop_recording_for(operation)?;
        return Err(AppError::Cancelled("Диктовка отменена".into()));
    }
    #[cfg(feature = "speech-vad")]
    if !force_standard
        && pipeline
            .current_operation()
            .filter(|current| current.id == operation)
            .is_some_and(|operation| {
                matches!(
                    operation.source,
                    OperationSource::Ui | OperationSource::Hotkey
                )
            })
    {
        if let Err(error) = super::capture_segments::maybe_start(app, operation, settings) {
            tracing::warn!(operation, %error, "capture pretranscription unavailable; ordinary Stop remains available");
        }
    }
    Ok(())
}

fn activate_recording(
    app: AppHandle,
    operation: u64,
    source: OperationSource,
    command: bool,
) -> AppResult<u64> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    super::workflow::capture_target(&app, operation, source);
    // The focus probe above deliberately runs outside all admission/capture
    // locks. Cancel may finish this operation while its provider is responding.
    // An old start must not pause WakeWord after idle cleanup already resumed it.
    if !super::capture_activation::activate(
        || {
            pipeline
                .while_operation(operation, || {
                    crate::application::wake_runtime::pause(&app);
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
    arm_recording_safety_timeout(app.clone(), operation, command);
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
            if let Err(error) = crate::application::voice_dictation::run(&app, operation).await {
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

pub(super) enum CapturedSamples {
    Speech(Vec<i16>),
    Finished(super::DictationOutcome),
}

pub(super) fn finish_samples(
    session: &Session,
    diagnostic: &mut DictationTailDiagnostic,
    report: &crate::application::diagnostic_report::telemetry::ReportTrace,
) -> AppResult<CapturedSamples> {
    let pipeline = session.app.state::<Pipeline>();
    // Stop and drain the audio subscriber before loading a model.
    let samples = pipeline.stop_recording_for(session.operation);
    let capture_drops = pipeline.capture_dropped_packets_for(session.operation);
    diagnostic.record_capture_drops(capture_drops);
    report.capture_dropped_packets(capture_drops);
    let samples = samples.map_err(|error| {
        diagnostic.emit();
        session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
        error
    })?;
    let Some(samples) = samples else {
        diagnostic.stt_cancelled();
        return Ok(CapturedSamples::Finished(session.interrupted()));
    };
    diagnostic.record_capture(samples.len());
    if pipeline.recording_limit_reached() {
        emit_pipeline_error(&session.app, &format!("Recording reached the {} second safety limit; the captured part will be transcribed.", pipeline::MAX_RECORDING_SECONDS));
    }
    if !session.active("after recording") {
        diagnostic.stt_cancelled();
        return Ok(CapturedSamples::Finished(session.interrupted()));
    }
    if samples.is_empty() {
        return Ok(CapturedSamples::Finished(super::DictationOutcome::NoSpeech));
    }
    let mut speech_gate = crate::application::speech_gate::SpeechGate::new(&session.app)?;
    let Some(speech) =
        speech_gate.analyze_recording(&samples, || session.cancellation.is_cancelled())
    else {
        return Ok(CapturedSamples::Finished(session.interrupted()));
    };
    if !speech.has_speech {
        tracing::debug!(
            operation = session.operation,
            captured_samples = samples.len(),
            "dictation skipped: no confirmed speech in capture"
        );
        return Ok(CapturedSamples::Finished(
            if session.cancellation.is_cancelled() || !session.active("speech validation stopped") {
                session.interrupted()
            } else {
                super::DictationOutcome::NoSpeech
            },
        ));
    }
    if !session.active("after speech validation") {
        diagnostic.stt_cancelled();
        return Ok(CapturedSamples::Finished(session.interrupted()));
    }
    let captured_samples = samples.len();
    // Preserve the complete onset and tail after measured trimming regressions.
    // Wake handoff continues to keep its exact original buffer.
    let (trimmed, vad) = crate::vad::prepare_recording_with_result(&samples, &speech);
    let samples = if session.source == OperationSource::WakeWord {
        diagnostic.skip_vad(captured_samples);
        samples
    } else {
        let retained_trailing = captured_samples.saturating_sub(vad.speech_end_sample);
        diagnostic.record_vad(captured_samples, trimmed.len(), &vad, retained_trailing);
        trimmed
    };
    if samples.is_empty() {
        return Ok(CapturedSamples::Finished(super::DictationOutcome::NoSpeech));
    }
    if !session.transition(PipelineState::Transcribing, TerminalReason::Completed) {
        diagnostic.stt_cancelled();
        return Ok(CapturedSamples::Finished(session.interrupted()));
    }
    Ok(CapturedSamples::Speech(samples))
}
