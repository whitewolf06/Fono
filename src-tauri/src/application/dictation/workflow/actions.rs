//! User decisions are fenced by the captured session, including delayed insertion.
use super::{
    publish, state::Claim, PendingAction, PendingData, PendingDictation, PendingRequest, Runtime,
};
use crate::{
    application::{
        diagnostic_report::telemetry::{ReportOutcome, ReportStage},
        dictation::{empty_transcript, lifecycle::OperationScope, postprocess},
        dictation_result,
    },
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{AiMode, PipelineState, Transcript},
};
use fono_core::TerminalReason;
use tauri::{AppHandle, Manager};

pub(crate) async fn resolve(
    app: AppHandle,
    request: PendingRequest,
) -> AppResult<Option<Transcript>> {
    if request.action == PendingAction::Cancel {
        return cancel(&app, request.session_id);
    }
    let claim = app.state::<Runtime>().store.lock().claim(&request)?;
    let Claim::Work(data, choice) = claim else {
        return match claim {
            Claim::Completed(output) => Ok(output),
            _ => unreachable!(),
        };
    };
    let mut scope = OperationScope::new(app.clone(), request.session_id, false);
    let mut guard = ActionGuard {
        session: data.session.clone(),
        settled: false,
        insertion_started: false,
    };
    publish(&app);
    let result = execute(&data, &choice, request.action, &mut guard).await;
    data.report.outcome(match &result {
        Ok(_) if data.session.active("pending completed report") => ReportOutcome::Completed,
        Ok(_) => ReportOutcome::Cancelled,
        Err(error) => ReportOutcome::from_error(error),
    });
    guard.settled = true;
    scope.complete(&result);
    result.map(|output| (!output.text.is_empty()).then_some(output))
}

fn cancel(app: &AppHandle, id: u64) -> AppResult<Option<Transcript>> {
    let current = super::snapshot(app);
    if current.as_ref().map(|s| s.session_id) != Some(id) {
        return match app.state::<Runtime>().store.lock().claim(&PendingRequest {
            session_id: id,
            action: PendingAction::Cancel,
            preset: None,
            target_language: None,
        })? {
            Claim::Completed(_) => Ok(None),
            Claim::Work(..) => unreachable!(),
        };
    }
    let pipeline = app.state::<Pipeline>();
    if let Some(event) = pipeline.cancel_for(id) {
        crate::events::emit_operation(app, event);
    }
    super::cancelled(app, id);
    pipeline::set_idle_if_no_operation(app, app.state::<AppState>().inner(), &pipeline);
    crate::application::dictation::resume_wake_if_idle(app);
    Ok(None)
}

async fn execute(
    data: &PendingData,
    choice: &PendingDictation,
    action: PendingAction,
    guard: &mut ActionGuard,
) -> AppResult<Transcript> {
    let session = &data.session;
    if !session.active("pending action start") {
        return Ok(empty_transcript());
    }
    let final_text = match action {
        PendingAction::InsertRaw => data.transcript.text.clone(),
        PendingAction::Complete => choice
            .result_text
            .clone()
            .unwrap_or_else(|| data.transcript.text.clone()),
        PendingAction::ProcessAndInsert | PendingAction::ProcessPreview => {
            let _timer = data.report.stage(ReportStage::Processing);
            match postprocess::selected(
                session,
                &data.transcript,
                choice.preset,
                if choice.translation_enabled {
                    choice.target_language
                } else {
                    None
                },
                choice.processing_enabled,
            )
            .await
            {
                Ok(Some(text)) => text,
                Ok(None) => return Ok(empty_transcript()),
                Err(error) => {
                    retry(session, error.to_string(), false);
                    return Err(error);
                }
            }
        }
        PendingAction::Cancel => unreachable!(),
    };
    let final_text = if action == PendingAction::Complete && choice.result_text.is_some() {
        final_text
    } else {
        crate::application::personal_dictionary::canonicalize_dictation(
            &session.settings,
            &final_text,
        )
    };
    if !session.active("pending before insertion") {
        return Ok(empty_transcript());
    }
    let pipeline = session.app.state::<Pipeline>();
    if pipeline
        .while_operation(session.operation, || {
            session
                .app
                .state::<Runtime>()
                .store
                .lock()
                .generated(session.operation, final_text.clone());
            dictation_result::publish(
                &session.app,
                &data.history_id,
                &data.transcript,
                &final_text,
            );
        })
        .is_none()
    {
        return Ok(empty_transcript());
    }
    publish(&session.app);
    let timings = if action == PendingAction::Complete {
        data.ready_timings
            .clone()
            .unwrap_or_else(|| data.report.history_timings())
    } else {
        data.report.history_timings()
    };
    if action == PendingAction::ProcessPreview {
        super::preview_result(&session.app, session.operation, final_text.clone());
        session.transition(PipelineState::AwaitingAction, TerminalReason::Completed);
        return Ok(Transcript {
            text: final_text,
            ..data.transcript.clone()
        });
    }
    if action != PendingAction::Complete && session.source != fono_core::OperationSource::Ui {
        let Some(target) = data
            .target
            .as_ref()
            .filter(|target| crate::injection::target::matches(target))
        else {
            let error = AppError::Injection("Поле для вставки изменилось. Скопируйте текст из Fono или отмените и начните диктовку в нужном поле".into());
            retry(session, error.to_string(), true);
            return Err(error);
        };
        if !session.transition(PipelineState::Injecting, TerminalReason::Completed) {
            return Ok(empty_transcript());
        }
        guard.insertion_started = true;
        let _timer = data.report.stage(ReportStage::Insertion);
        let insertion = super::insertion::insert(session, Some(target), &final_text).await;
        if !session.active("pending insertion returned") {
            return Ok(empty_transcript());
        }
        match insertion {
            Ok(()) => (),
            Err(error) => {
                retry(session, error.to_string(), true);
                return Err(error);
            }
        }
    }
    let output = Transcript {
        text: final_text.clone(),
        ..data.transcript.clone()
    };
    // Finishing ownership and archiving share the operation fence. Exactly one
    // action can reach this point; Cancel prevents both late publication/archive.
    let completed = pipeline.while_operation(session.operation, || {
        let detached = session
            .app
            .state::<Runtime>()
            .store
            .lock()
            .detach(session.operation, Some(output.clone()));
        if detached.is_some() {
            let mut settings = session.settings.clone();
            settings.processing_preset = Some(choice.preset);
            settings.processing_target_language = choice.target_language;
            settings.processing_translation_enabled = choice.translation_enabled;
            if action == PendingAction::InsertRaw || !choice.processing_enabled {
                settings.ai_mode = AiMode::Off;
                settings.processing_target_language = None;
            } else {
                settings.ai_mode = if choice.preset == super::TextPreset::Format {
                    AiMode::Format
                } else {
                    AiMode::Clean
                };
            }
            dictation_result::archive_session(
                &session.app,
                &settings,
                dictation_result::SessionArchive {
                    id: data.history_id.clone(),
                    transcript: &data.transcript,
                    final_text: &final_text,
                    operation: session.operation,
                    timings,
                    created_at: data.created_at,
                },
            );
        }
        drop(detached);
    });
    if completed.is_none() {
        return Ok(empty_transcript());
    }
    publish(&session.app);
    Ok(output)
}

fn retry(session: &super::Session, error: String, blocked: bool) {
    if session
        .app
        .state::<Runtime>()
        .store
        .lock()
        .retry(session.operation, error, blocked)
    {
        session.transition(PipelineState::AwaitingAction, TerminalReason::Completed);
        publish(&session.app);
    }
}

struct ActionGuard {
    session: super::Session,
    settled: bool,
    insertion_started: bool,
}
impl Drop for ActionGuard {
    fn drop(&mut self) {
        if !self.settled && self.session.active("abandoned pending action") {
            if self.insertion_started {
                // A dropped IPC future must also stop its blocking send worker.
                self.session.cancellation.cancel();
            }
            retry(
                &self.session,
                "Действие прервано. Исходный текст сохранён".into(),
                self.insertion_started,
            );
        }
    }
}
