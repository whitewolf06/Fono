//! Idempotent ordinary stop publishes raw text before optional processing.
use super::{
    capture,
    lifecycle::{OperationScope, Session},
    postprocess, recognition, DictationOutcome,
};
use crate::{
    application::{
        diagnostic_report::telemetry::{ReportOutcome, ReportStage, ReportTrace},
        dictation_result,
        dictation_tail_diagnostics::{DictationStopReason, DictationTailDiagnostic},
    },
    error::AppResult,
    pipeline::{
        completion::{wait, FinishClaim},
        Pipeline,
    },
    types::{PipelineState, Transcript},
};
use fono_core::{OperationSource, TerminalReason};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub(crate) async fn stop(app: AppHandle) -> AppResult<DictationOutcome> {
    super::stop_with_reason(app, DictationStopReason::Manual).await
}

pub(crate) async fn stop_with_reason(
    app: AppHandle,
    reason: DictationStopReason,
) -> AppResult<DictationOutcome> {
    #[cfg(feature = "experimental-live")]
    if crate::application::live_dictation::is_active(&app)
        || (app.state::<Pipeline>().operation_id() == 0
            && crate::application::live_dictation::snapshot(&app).is_some())
    {
        return crate::application::live_dictation::finish(app)
            .await
            .map(DictationOutcome::Completed);
    }
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.operation_id();
    stop_operation(app, operation, reason, false).await
}

/// Timers and key releases retain the operation they actually started.
pub(crate) async fn stop_with_reason_for(
    app: AppHandle,
    operation: u64,
    reason: DictationStopReason,
) -> AppResult<DictationOutcome> {
    if !app.state::<Pipeline>().is_operation_active(operation) {
        return Ok(DictationOutcome::interrupted(
            &app.state::<Pipeline>(),
            operation,
        ));
    }
    stop_operation(app, operation, reason, false).await
}

pub(crate) async fn stop_overlay_for(
    app: AppHandle,
    operation: u64,
) -> AppResult<DictationOutcome> {
    stop_operation(app, operation, DictationStopReason::Manual, true).await
}

async fn stop_operation(
    app: AppHandle,
    operation: u64,
    reason: DictationStopReason,
    copy_only: bool,
) -> AppResult<DictationOutcome> {
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
            let report = Arc::new(ReportTrace::begin(operation, source));
            let mut result = stop_once(
                &app,
                operation,
                reason,
                &report,
                copy_only,
                scope.settlement_receipt(),
            )
            .await;
            scope.complete_outcome(&mut result);
            report.outcome(ReportOutcome::from_dictation_result(&result));
            drop(scope);
            if let Ok(outcome) = &result {
                crate::events::emit_dictation_outcome(&app, operation, outcome.kind());
            }
            ticket.complete(&result);
            result
        }
    }
}

async fn stop_once(
    app: &AppHandle,
    operation: u64,
    reason: DictationStopReason,
    report: &Arc<ReportTrace>,
    copy_only: bool,
    receipt: super::SettlementReceipt,
) -> AppResult<DictationOutcome> {
    let session = match Session::current(app, operation) {
        Ok(session) => session,
        Err(crate::error::AppError::Cancelled(_)) => {
            return Ok(DictationOutcome::interrupted(
                &app.state::<Pipeline>(),
                operation,
            ))
        }
        Err(error) => return Err(error),
    };
    let diagnostic = DictationTailDiagnostic::new(operation, session.source, reason);
    let (samples, mut diagnostic) =
        capture::finish_samples_async(session.clone(), diagnostic, report.clone()).await?;
    let samples = match samples {
        capture::CapturedSamples::Speech(samples) => samples,
        capture::CapturedSamples::Finished(outcome) => return Ok(outcome),
    };
    report.captured(diagnostic.captured_samples().unwrap_or(samples.len()));
    let transcript = match recognition::run_measured(&session, samples, Some(report)).await {
        Ok(Some(transcript)) => transcript,
        Ok(None) => {
            diagnostic.stt_cancelled();
            return Ok(session.interrupted());
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
    #[cfg(feature = "wake-word")]
    let mut transcript = transcript;
    #[cfg(feature = "wake-word")]
    if session.source == OperationSource::WakeWord {
        transcript.text = crate::application::wake::strip_leading_wake_phrase(
            &transcript.text,
            &session.settings.wake_word,
        );
        if let Some(command) = crate::application::wake::extract_wake_command(&transcript.text) {
            crate::application::wake_dictation::propose_command(app, operation, command);
            return Ok(DictationOutcome::Deferred(transcript));
        }
    }
    if transcript.text.trim().is_empty() {
        return Ok(DictationOutcome::NoSpeech);
    }
    let history_id = crate::history::next_id();
    let pipeline = app.state::<Pipeline>();
    if pipeline
        .while_operation(operation, || {
            dictation_result::publish(app, &history_id, &transcript, &transcript.text);
        })
        .is_none()
    {
        return Ok(session.interrupted());
    }
    if !copy_only
        && session.source != OperationSource::Hotkey
        && super::workflow::should_defer(&session.settings)
    {
        super::workflow::enqueue(
            &session,
            &transcript,
            history_id,
            report.clone(),
            None,
            false,
        )?;
        return Ok(DictationOutcome::Deferred(transcript));
    }
    let processed = {
        let _timer = report.stage(ReportStage::Processing);
        postprocess::selected(
            &session,
            &transcript,
            super::workflow::effective_preset(&session.settings),
            super::workflow::effective_language(&session.settings),
            super::workflow::should_process(&session.settings),
        )
        .await
    };
    let processed = match processed {
        Ok(value) => value,
        Err(error) => {
            diagnostic.postprocessor_fallback();
            session.error(crate::events::ErrorCodeV1::Llm, error.to_string());
            super::workflow::enqueue(
                &session,
                &transcript,
                history_id,
                report.clone(),
                Some(error.to_string()),
                copy_only,
            )?;
            if copy_only {
                super::workflow::preview_result(app, operation, transcript.text.clone());
            }
            return Ok(DictationOutcome::Deferred(transcript));
        }
    };
    let Some(final_text) = processed else {
        return Ok(session.interrupted());
    };
    if session.settings.ai_mode == crate::types::AiMode::Off {
        diagnostic.postprocessor_skipped();
    } else {
        diagnostic.postprocessor_succeeded(final_text != transcript.text);
    }
    let final_text = crate::application::personal_dictionary::canonicalize_dictation(
        &session.settings,
        &final_text,
    );
    let history_timings = report.history_timings();
    if !session.active("before injection") {
        return Ok(session.interrupted());
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
        return Ok(session.interrupted());
    }
    if copy_only {
        super::workflow::enqueue(
            &session,
            &transcript,
            history_id,
            report.clone(),
            None,
            true,
        )?;
        super::workflow::preview_result(app, operation, final_text.clone());
        return Ok(DictationOutcome::Deferred(Transcript {
            text: final_text,
            ..transcript
        }));
    }
    let insertion = if session.source != OperationSource::Ui {
        if !session.transition(PipelineState::Injecting, TerminalReason::Completed) {
            return Ok(session.interrupted());
        }
        // Preflight is fenced; native SendInput/clipboard must remain outside
        // the capture lock so cancellation can stop an operation promptly.
        if pipeline.while_operation(operation, || !session.cancellation.is_cancelled())
            != Some(true)
        {
            return Ok(session.interrupted());
        }
        let _timer = report.stage(ReportStage::Insertion);
        super::workflow::insert_captured(&session, &final_text).await
    } else {
        Ok(())
    };
    if !session.active("ordinary insertion returned") {
        return Ok(session.interrupted());
    }
    if let Err(error) = insertion {
        session.error(crate::events::ErrorCodeV1::Injection, error.to_string());
        super::workflow::enqueue(
            &session,
            &transcript,
            history_id,
            report.clone(),
            Some(error.to_string()),
            false,
        )?;
        super::workflow::block_insertion(app, operation, error.to_string(), final_text.clone());
        return Ok(DictationOutcome::Deferred(Transcript {
            text: final_text,
            ..transcript
        }));
    }
    let archive_settings = session.settings.clone();
    let archive_transcript = transcript.clone();
    let archive_text = final_text.clone();
    if dictation_result::commit_for_operation(app.clone(), operation, receipt, move |app| {
        dictation_result::archive_with_metadata(
            app,
            &archive_settings,
            history_id,
            &archive_transcript,
            &archive_text,
            operation,
            history_timings,
        );
    })
    .await?
    .is_none()
    {
        return Ok(session.interrupted());
    }
    Ok(DictationOutcome::Completed(Transcript {
        text: final_text,
        ..transcript
    }))
}
