//! A failed remote attempt retains PCM and its original operation until a decision.
mod attempts;
mod state;

use super::{
    lifecycle::{OperationScope, Session},
    recognition, standard, DictationOutcome, SettlementReceipt,
};
use crate::{
    application::{
        diagnostic_report::telemetry::{ReportOutcome, ReportTrace},
        dictation_tail_diagnostics::DictationTailDiagnostic,
        updates::activity::{self, ActivityLease},
    },
    error::{AppError, AppResult},
    pipeline::{self, remote_check::RemoteCheckState, Pipeline},
    state::AppState,
    types::{PipelineState, RemoteFallback, Settings},
};
use fono_core::TerminalReason;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryPhase {
    AwaitingAction,
    Recognizing,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryReason {
    ServerFailed,
    ServerUnavailable,
    LocalFailed,
    ChooseSource,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecoverySnapshot {
    pub session_id: u64,
    pub phase: RecoveryPhase,
    pub reason: RecoveryReason,
    pub audio_secs: f64,
    pub local_available: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry,
    RecognizeLocal,
    Cancel,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRequest {
    pub session_id: u64,
    pub action: RecoveryAction,
}

#[derive(Clone)]
struct Data {
    session: Session,
    samples: Arc<Vec<i16>>,
    diagnostic: Arc<Mutex<Option<DictationTailDiagnostic>>>,
    report: Arc<ReportTrace>,
    copy_only: bool,
    local: Arc<Mutex<Option<Settings>>>,
    _activity: Arc<ActivityLease<'static>>,
}
#[derive(Default)]
pub(crate) struct Runtime {
    store: Mutex<state::Store<Data>>,
}

pub(crate) fn snapshot(app: &AppHandle) -> Option<RecoverySnapshot> {
    app.try_state::<Runtime>().and_then(|runtime| {
        runtime
            .store
            .lock()
            .record
            .as_ref()
            .map(|record| record.snapshot.clone())
    })
}
pub(crate) fn has_pending(app: &AppHandle) -> bool {
    snapshot(app).is_some()
}
pub(crate) fn retains(app: &AppHandle, operation: u64) -> bool {
    snapshot(app).is_some_and(|snapshot| snapshot.session_id == operation)
}
fn publish(app: &AppHandle) {
    let _ = app.emit("recovery-dictation", snapshot(app));
}

pub(crate) fn cancelled(app: &AppHandle, operation: u64) {
    let detached = app.try_state::<Runtime>().and_then(|runtime| {
        runtime
            .store
            .lock()
            .detach(operation, Some(DictationOutcome::Cancelled))
    });
    let changed = detached.is_some();
    if let Some(data) = &detached {
        data.report.outcome(ReportOutcome::Cancelled);
    }
    drop(detached);
    if changed {
        publish(app);
    }
}
pub(crate) fn shutdown(app: &AppHandle) {
    if let Some(current) = snapshot(app) {
        cancelled(app, current.session_id);
    }
}

/// Called after the one capture/VAD drain and before any remote model I/O.
pub(super) async fn start(
    session: Session,
    samples: Vec<i16>,
    diagnostic: DictationTailDiagnostic,
    report: Arc<ReportTrace>,
    copy_only: bool,
    receipt: SettlementReceipt,
) -> AppResult<DictationOutcome> {
    validate_sample_count(samples.len())?;
    let activity = Arc::new(activity::lease()?);
    let data = Data {
        session: session.clone(),
        samples: Arc::new(samples),
        diagnostic: Arc::new(Mutex::new(Some(diagnostic))),
        report,
        copy_only,
        local: Arc::new(Mutex::new(None)),
        _activity: activity,
    };
    let initial_reason = if server_unavailable(&session) {
        RecoveryReason::ServerUnavailable
    } else {
        RecoveryReason::ChooseSource
    };
    let installed = session
        .app
        .state::<Pipeline>()
        .while_operation(session.operation, || {
            session.app.state::<Runtime>().store.lock().install(
                RecoverySnapshot {
                    session_id: session.operation,
                    phase: RecoveryPhase::Recognizing,
                    reason: initial_reason,
                    audio_secs: data.samples.len() as f64 / 16_000.0,
                    local_available: false,
                },
                data.clone(),
            )
        })
        .transpose()?;
    if installed.is_none() {
        return Ok(session.interrupted());
    }
    let mut guard = AttemptGuard::new(&data, initial_reason);
    prepare_local(&data).await;
    if !session.active("recovery prepared local choice") {
        return Ok(session.interrupted());
    }
    let policy = session
        .settings
        .remote_stt
        .as_ref()
        .map(|remote| remote.fallback.clone())
        .unwrap_or_default();
    let plan = initial_attempt(&policy, server_unavailable(&session));
    let result = match plan {
        InitialAttempt::Ask => {
            pause(&data, initial_reason);
            Ok(DictationOutcome::RecoveryPending)
        }
        InitialAttempt::Local => attempt(&data, true, false, receipt).await,
        InitialAttempt::Remote => {
            attempt(
                &data,
                false,
                matches!(policy, RemoteFallback::Local(_)),
                receipt,
            )
            .await
        }
    };
    guard.settled = true;
    result
}

fn validate_sample_count(count: usize) -> AppResult<()> {
    if count > 16_000 * pipeline::MAX_RECORDING_SECONDS {
        return Err(AppError::Audio(
            "Запись превышает ограничение в пять минут".into(),
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialAttempt {
    Ask,
    Local,
    Remote,
}
fn initial_attempt(policy: &RemoteFallback, unavailable: bool) -> InitialAttempt {
    match policy {
        RemoteFallback::Ask => InitialAttempt::Ask,
        RemoteFallback::Local(_) if unavailable => InitialAttempt::Local,
        _ => InitialAttempt::Remote,
    }
}
fn server_unavailable(session: &Session) -> bool {
    session.remote_check.as_ref().is_some_and(|check| {
        matches!(
            check.state,
            RemoteCheckState::Starting
                | RemoteCheckState::Stopping
                | RemoteCheckState::ModelNotReady
                | RemoteCheckState::Unavailable { .. }
        )
    })
}
async fn prepare_local(data: &Data) {
    let settings = data.session.settings.clone();
    let activity = data._activity.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let model = match settings.remote_stt.as_ref().map(|remote| &remote.fallback) {
            Some(RemoteFallback::Local(id)) => Some(id),
            _ => None,
        };
        settings.local_fallback_settings(model)
    });
    if let Ok(Ok(settings)) = task.await {
        let session = &data.session;
        session
            .app
            .state::<Pipeline>()
            .while_operation(session.operation, || {
                let runtime = session.app.state::<Runtime>();
                let mut store = runtime.store.lock();
                if let Some(record) = store
                    .record
                    .as_mut()
                    .filter(|record| record.snapshot.session_id == session.operation)
                {
                    *data.local.lock() = Some(settings);
                    record.snapshot.local_available = true;
                }
            });
    }
}

pub(crate) async fn resolve(
    app: AppHandle,
    request: RecoveryRequest,
) -> AppResult<DictationOutcome> {
    if request.action == RecoveryAction::Cancel {
        return cancel(&app, request.session_id);
    }
    let pipeline = app.state::<Pipeline>();
    // The Stop owner must publish its nonterminal decision before a very fast
    // recovery can commit success. Otherwise Stop could emit Cancelled after
    // the retry already retired the operation. A dropped Stop ticket also wakes
    // this waiter; its error does not consume the retained recording.
    if let Some(receiver) = pipeline.completion.existing(request.session_id) {
        let _ = crate::pipeline::completion::wait(receiver).await;
    }
    // The operation fence precedes the recovery slot, matching cancellation.
    let claim = pipeline.while_operation(request.session_id, || {
        app.state::<Runtime>()
            .store
            .lock()
            .claim(request.session_id)
    });
    let data = match claim {
        Some(Ok(Ok(data))) => data,
        Some(Ok(Err(outcome))) => return Ok(outcome),
        Some(Err(error)) => return Err(error),
        None => {
            return app
                .state::<Runtime>()
                .store
                .lock()
                .completed(request.session_id)
                .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()))
        }
    };
    let mut scope = OperationScope::new(app.clone(), request.session_id, false);
    let mut guard = AttemptGuard::new(&data, RecoveryReason::ServerFailed);
    if !data
        .session
        .transition(PipelineState::Processing, TerminalReason::Completed)
    {
        return Ok(data.session.interrupted());
    }
    publish(&app);
    // Retry means an explicit server attempt; it never silently changes source.
    let mut result = attempt(
        &data,
        request.action == RecoveryAction::RecognizeLocal,
        false,
        scope.settlement_receipt(),
    )
    .await;
    scope.complete_outcome(&mut result);
    data.report
        .outcome(ReportOutcome::from_dictation_result(&result));
    guard.settled = true;
    drop(scope);
    if let Ok(outcome) = &result {
        crate::events::emit_dictation_outcome(&app, request.session_id, outcome.kind());
    }
    result
}

fn cancel(app: &AppHandle, operation: u64) -> AppResult<DictationOutcome> {
    let pipeline = app.state::<Pipeline>();
    if !retains(app, operation) {
        return app
            .state::<Runtime>()
            .store
            .lock()
            .completed(operation)
            .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()));
    }
    let Some(event) = pipeline.cancel_for(operation) else {
        return app
            .state::<Runtime>()
            .store
            .lock()
            .completed(operation)
            .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()));
    };
    crate::events::emit_operation(app, event);
    super::workflow::cancelled(app, operation);
    pipeline::set_idle_if_no_operation(app, app.state::<AppState>().inner(), &pipeline);
    super::resume_wake_if_idle(app);
    crate::events::emit_dictation_outcome(app, operation, super::DictationOutcomeKind::Cancelled);
    Ok(DictationOutcome::Cancelled)
}

async fn attempt(
    data: &Data,
    local: bool,
    auto_local: bool,
    receipt: SettlementReceipt,
) -> AppResult<DictationOutcome> {
    use attempts::Source;
    let result = attempts::run(
        if local { Source::Local } else { Source::Server },
        auto_local,
        |source| {
            let mut session = data.session.clone();
            async move {
                if !session.active("recovery attempt start") || session.cancellation.is_cancelled()
                {
                    return Ok(None);
                }
                if source == Source::Local {
                    session.settings = data
                        .local
                        .lock()
                        .clone()
                        .ok_or(AppError::ModelNotSelected)?;
                }
                match recognition::run_shared_measured(
                    &session,
                    data.samples.clone(),
                    Some(&data.report),
                )
                .await
                {
                    Ok(result) => Ok(result.map(|transcript| (session, transcript))),
                    Err(_)
                        if session.cancellation.is_cancelled()
                            || !session.active("recovery failed after cancellation") =>
                    {
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
        },
    )
    .await;
    let (session, transcript) = match result {
        Ok(Some((result, _))) => result,
        Ok(None) => return Ok(data.session.interrupted()),
        Err(source) => {
            if let Some(diagnostic) = data.diagnostic.lock().as_mut() {
                diagnostic.stt_failed();
            }
            pause(
                data,
                if source == Source::Local {
                    RecoveryReason::LocalFailed
                } else {
                    RecoveryReason::ServerFailed
                },
            );
            return Ok(DictationOutcome::RecoveryPending);
        }
    };
    if !session.active("recovery before result handoff") {
        return Ok(session.interrupted());
    }
    let diagnostic = data.diagnostic.lock().take().expect("one recovery handoff");
    let detached = session
        .app
        .state::<Runtime>()
        .store
        .lock()
        .detach(session.operation, None);
    drop(detached);
    publish(&session.app);
    let outcome = standard::complete_transcript(
        session.clone(),
        transcript,
        diagnostic,
        &data.report,
        data.copy_only,
        receipt,
    )
    .await;
    if let Ok(outcome) = &outcome {
        session
            .app
            .state::<Runtime>()
            .store
            .lock()
            .complete(session.operation, outcome.clone());
    }
    outcome
}
fn pause(data: &Data, reason: RecoveryReason) {
    let session = &data.session;
    // Publish the coordinator's waiting phase before making a new claim
    // possible; an immediate retry must not be moved back to AwaitingAction.
    if !session.transition(PipelineState::AwaitingAction, TerminalReason::Completed) {
        return;
    }
    let changed = session
        .app
        .state::<Pipeline>()
        .while_operation(session.operation, || {
            session
                .app
                .state::<Runtime>()
                .store
                .lock()
                .retry(session.operation, reason)
        });
    if changed == Some(true) {
        publish(&session.app);
    }
}
struct AttemptGuard {
    data: Data,
    reason: RecoveryReason,
    settled: bool,
}
impl AttemptGuard {
    fn new(data: &Data, reason: RecoveryReason) -> Self {
        Self {
            data: data.clone(),
            reason,
            settled: false,
        }
    }
}
impl Drop for AttemptGuard {
    fn drop(&mut self) {
        if !self.settled && retains(&self.data.session.app, self.data.session.operation) {
            pause(&self.data, self.reason);
        }
        // Settlement and publication keep ownership even after a waiting
        // choice becomes visible to a second window.
        self.data
            .session
            .app
            .state::<Runtime>()
            .store
            .lock()
            .release(self.data.session.operation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_audio_is_bounded_by_the_existing_capture_limit() {
        assert!(validate_sample_count(16_000 * 300).is_ok());
        assert!(validate_sample_count(16_000 * 300 + 1).is_err());
    }
}
