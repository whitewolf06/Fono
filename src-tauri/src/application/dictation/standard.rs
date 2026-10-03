//! Idempotent ordinary stop publishes raw text before optional processing.
use super::{
    capture, empty_transcript,
    lifecycle::{OperationScope, Session},
    postprocess, recognition,
};
use crate::{
    application::{
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
            let mut scope = OperationScope::new(app.clone(), operation, false);
            let result = stop_once(&app, operation, reason).await;
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
) -> AppResult<Transcript> {
    let session = Session::current(app, operation)?;
    let mut diagnostic = DictationTailDiagnostic::new(operation, session.source, reason);
    let Some(samples) = capture::finish_samples(&session, &mut diagnostic)? else {
        return Ok(empty_transcript());
    };
    let mut transcript = match recognition::run(&session, samples).await {
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
    let Some(final_text) = postprocess::process(&session, &transcript, Some(&mut diagnostic)).await
    else {
        return Ok(empty_transcript());
    };
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
        crate::injection::inject_text(&final_text, session.settings.injection_mode)
    } else {
        Ok(())
    };
    if pipeline
        .while_operation(operation, || {
            dictation_result::archive(
                app,
                &session.settings,
                history_id,
                &transcript,
                &final_text,
                operation,
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
