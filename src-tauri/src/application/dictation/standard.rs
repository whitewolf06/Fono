//! Idempotent ordinary stop publishes raw text before optional processing.
use super::{
    capture, empty_transcript,
    lifecycle::{OperationScope, Session},
    postprocess, recognition,
};
use crate::{
    application::{
        diagnostic_report::telemetry::{ReportOutcome, ReportStage, ReportTrace},
        dictation_result,
        dictation_tail_diagnostics::{DictationStopReason, DictationTailDiagnostic},
    },
    error::AppResult,
    operation::{OperationSource, TerminalReason},
    pipeline::{
        completion::{wait, FinishClaim},
        Pipeline,
    },
    types::{PipelineState, Transcript},
};
use tauri::{AppHandle, Manager};

pub(crate) async fn stop(app: AppHandle) -> AppResult<Transcript> {
    super::stop_with_reason(app, DictationStopReason::Manual).await
}

pub(crate) async fn stop_with_reason(
    app: AppHandle,
    reason: DictationStopReason,
) -> AppResult<Transcript> {
    if crate::application::live_dictation::is_active(&app)
        || (app.state::<Pipeline>().operation_id() == 0
            && crate::application::live_dictation::snapshot(&app).is_some())
    {
        return crate::application::live_dictation::finish(app).await;
    }
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.operation_id();
    stop_operation(app, operation, reason).await
}

/// Timers and key releases retain the operation they actually started.
pub(crate) async fn stop_with_reason_for(
    app: AppHandle,
    operation: u64,
    reason: DictationStopReason,
) -> AppResult<Transcript> {
    if !app.state::<Pipeline>().is_operation_active(operation) {
        return Ok(empty_transcript());
    }
    stop_operation(app, operation, reason).await
}

async fn stop_operation(
    app: AppHandle,
    operation: u64,
    reason: DictationStopReason,
) -> AppResult<Transcript> {
    let pipeline = app.state::<Pipeline>();
    match pipeline.completion.claim(operation)? {
        FinishClaim::Waiting(receiver) => wait(receiver).await,
        FinishClaim::Owner(ticket) => {
            let _activity = crate::application::updates::activity::lease()?;
            let mut scope = OperationScope::new(app.clone(), operation, false);
            let source = pipeline
                .current_operation()
                .map(|item| item.source)
                .unwrap_or(OperationSource::Ui);
            let report = ReportTrace::begin(operation, source);
            let result = stop_once(&app, operation, reason, &report).await;
            report.outcome(match &result {
                Ok(transcript)
                    if transcript.text.is_empty() && pipeline.is_operation_active(operation) =>
                {
                    ReportOutcome::Empty
                }
                Ok(transcript) if transcript.text.is_empty() => ReportOutcome::Cancelled,
                Ok(_) => ReportOutcome::Completed,
                Err(error) => ReportOutcome::from_error(error),
            });
            scope.complete(&result);
            drop(scope);
            ticket.complete(&result);
            result
        }
    }
}

async fn stop_once(
    app: &AppHandle,
    operation: u64,
    reason: DictationStopReason,
    report: &ReportTrace,
) -> AppResult<Transcript> {
    let session = Session::current(app, operation)?;
    let mut diagnostic = DictationTailDiagnostic::new(operation, session.source, reason);
    let samples = {
        let _timer = report.stage(ReportStage::CaptureFinish);
        capture::finish_samples(&session, &mut diagnostic)?
    };
    let Some(samples) = samples else {
        return Ok(empty_transcript());
    };
    report.captured(diagnostic.captured_samples().unwrap_or(samples.len()));
    let mut transcript = match recognition::run_measured(&session, samples, Some(report)).await {
        Ok(Some(transcript)) => transcript,
        Ok(None) => {
            diagnostic.stt_cancelled();
            return Ok(empty_transcript());
        }
        Err(error) => {
            diagnostic.stt_failed();
            session.error(crate::events::ErrorCodeV1::Stt, error.to_string());
            return Err(error);
        }
    };
    diagnostic.stt_succeeded(!transcript.text.trim().is_empty());
    tracing::info!(
        "transcript ready ({} chars)",
        transcript.text.chars().count()
    );
    if session.source == OperationSource::WakeWord {
        transcript.text =
            crate::strip_leading_wake_phrase(&transcript.text, &session.settings.wake_word);
        if let Some(command) = crate::extract_wake_command(&transcript.text) {
            crate::application::wake_dictation::propose_command(app, operation, command);
            return Ok(transcript);
        }
    }
    let history_id = crate::history::next_id();
    let pipeline = app.state::<Pipeline>();
    if pipeline
        .while_operation(operation, || {
            dictation_result::publish(app, &history_id, &transcript, &transcript.text);
        })
        .is_none()
    {
        return Ok(empty_transcript());
    }
    let processed = {
        let _timer = report.stage(ReportStage::Processing);
        postprocess::process(&session, &transcript, Some(&mut diagnostic)).await
    };
    let Some(final_text) = processed else {
        return Ok(empty_transcript());
    };
    let final_text = crate::application::personal_dictionary::canonicalize_dictation(
        &session.settings,
        &final_text,
    );
    let history_timings = report.history_timings();
    if !session.active("before injection") {
        return Ok(empty_transcript());
    }
    crate::vlog!(
        "dictation final text ready ({} chars)",
        final_text.chars().count()
    );
    if pipeline
        .while_operation(operation, || {
            dictation_result::publish(app, &history_id, &transcript, &final_text);
        })
        .is_none()
    {
        return Ok(empty_transcript());
    }
    let insertion = if session.source != OperationSource::Ui {
        if !session.transition(PipelineState::Injecting, TerminalReason::Completed) {
            return Ok(empty_transcript());
        }
        // Preflight is fenced; native SendInput/clipboard must remain outside
        // the capture lock so cancellation can stop an operation promptly.
        if pipeline.while_operation(operation, || !session.cancellation.is_cancelled())
            != Some(true)
        {
            return Ok(empty_transcript());
        }
        let _timer = report.stage(ReportStage::Insertion);
        crate::injection::inject_text(&final_text, session.settings.injection_mode)
    } else {
        Ok(())
    };
    if pipeline
        .while_operation(operation, || {
            dictation_result::archive_with_metadata(
                app,
                &session.settings,
                history_id,
                &transcript,
                &final_text,
                operation,
                history_timings,
            );
        })
        .is_none()
    {
        return Ok(empty_transcript());
    }
    if let Err(error) = insertion {
        session.error(crate::events::ErrorCodeV1::Injection, error.to_string());
        return Err(error);
    }
    Ok(Transcript {
        text: final_text,
        ..transcript
    })
}
