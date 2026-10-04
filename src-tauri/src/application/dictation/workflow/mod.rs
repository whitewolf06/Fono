//! Deferred dictation owns its operation and updater lease until a user action.
mod actions;
mod insertion;
mod state;
mod types;

use super::lifecycle::Session;
use crate::application::diagnostic_report::telemetry::{ReportOutcome, ReportTrace};
use crate::{
    application::updates::activity::{self, ActivityLease},
    error::AppResult,
    injection::target::TextTarget,
    operation::{OperationSource, TerminalReason},
    pipeline::Pipeline,
    types::{AiMode, PipelineState, Transcript},
};
pub(crate) use actions::resolve;
use parking_lot::Mutex;
use state::{Record, Store};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use types::PendingPhase;
pub(crate) use types::{
    effective_preset, should_defer, should_process, PendingAction, PendingDictation, PendingRequest,
};
pub use types::{ProcessingWorkflow, TextPreset, TranslationLanguage};

#[derive(Clone)]
struct PendingData {
    session: Session,
    transcript: Transcript,
    history_id: String,
    created_at: chrono::DateTime<chrono::Utc>,
    report: Arc<ReportTrace>,
    target: Option<TextTarget>,
    _activity: Arc<ActivityLease<'static>>,
}

#[derive(Default)]
pub(crate) struct Runtime {
    store: Mutex<Store<PendingData>>,
    capture_target: Mutex<Option<(u64, Option<TextTarget>)>>,
}

pub(crate) fn snapshot(app: &AppHandle) -> Option<PendingDictation> {
    app.state::<Runtime>().store.lock().snapshot()
}
pub(crate) fn has_pending(app: &AppHandle) -> bool {
    app.state::<Runtime>().store.lock().pending.is_some()
}
pub(crate) fn is_busy(app: &AppHandle) -> bool {
    has_pending(app)
}
pub(super) fn retains(app: &AppHandle, operation: u64) -> bool {
    app.state::<Runtime>()
        .store
        .lock()
        .pending
        .as_ref()
        .is_some_and(|pending| pending.snapshot.session_id == operation)
}

/// Run before showing the overlay. Captures identity, never field contents.
pub(crate) fn capture_target(app: &AppHandle, operation: u64, source: OperationSource) {
    let target = if source == OperationSource::Ui {
        None
    } else {
        crate::injection::target::capture().ok()
    };
    // A late capture probe must not overwrite the target of a new operation.
    app.state::<Pipeline>().while_operation(operation, || {
        *app.state::<Runtime>().capture_target.lock() = Some((operation, target));
    });
}
pub(super) fn clear_target(app: &AppHandle, operation: u64) {
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.capture_target.lock();
    if slot.as_ref().is_some_and(|(id, _)| *id == operation) {
        *slot = None;
    }
}

pub(super) async fn insert_captured(session: &Session, text: &str) -> AppResult<()> {
    let target = session
        .app
        .state::<Runtime>()
        .capture_target
        .lock()
        .as_ref()
        .filter(|(id, _)| *id == session.operation)
        .and_then(|(_, target)| target.clone());
    insertion::insert(session, target.as_ref(), text).await
}

pub(super) fn block_insertion(app: &AppHandle, operation: u64, error: String, result: String) {
    let runtime = app.state::<Runtime>();
    let changed = {
        let mut store = runtime.store.lock();
        store.generated(operation, result);
        store.retry(operation, error, true)
    };
    if changed {
        publish(app);
    }
}

pub(super) fn enqueue(
    session: &Session,
    transcript: &Transcript,
    history_id: String,
    report: Arc<ReportTrace>,
    error: Option<String>,
) -> AppResult<bool> {
    let activity = Arc::new(activity::lease()?);
    let runtime = session.app.state::<Runtime>();
    let target = runtime
        .capture_target
        .lock()
        .as_ref()
        .filter(|(id, _)| *id == session.operation)
        .and_then(|(_, target)| target.clone());
    let unavailable = session.source != OperationSource::Ui && target.is_none();
    let created_at = chrono::Utc::now();
    let snapshot = PendingDictation {
        session_id: session.operation,
        phase: PendingPhase::AwaitingAction,
        original_text: transcript.text.clone(),
        result_text: None,
        created_at: created_at.to_rfc3339(),
        preset: effective_preset(&session.settings),
        target_language: session.settings.processing_target_language,
        processing_enabled: session.settings.ai_mode != AiMode::Off,
        source: session.source,
        error: error.or_else(|| unavailable.then(|| "Поле для вставки недоступно. Скопируйте текст из Fono или отмените и начните диктовку в нужном поле".into())),
        insertion_blocked: unavailable,
    };
    let data = PendingData {
        session: session.clone(),
        transcript: transcript.clone(),
        history_id,
        created_at,
        report,
        target,
        _activity: activity.clone(),
    };
    let installed = session
        .app
        .state::<Pipeline>()
        .while_operation(session.operation, || {
            runtime.store.lock().install(Record { snapshot, data })
        })
        .transpose()?;
    if installed.is_none() {
        return Ok(false);
    }
    if !session.transition(PipelineState::AwaitingAction, TerminalReason::Completed) {
        let detached = runtime.store.lock().detach(session.operation, None);
        drop(detached);
        return Ok(false);
    }
    publish(&session.app);
    Ok(true)
}

fn publish(app: &AppHandle) {
    if let Err(error) = app.emit("pending-dictation", snapshot(app)) {
        tracing::warn!(%error, "could not publish pending dictation");
    }
}

/// The caller cancels the operation first, so a late ASR cannot enqueue it again.
pub(crate) fn cancelled(app: &AppHandle, operation: u64) {
    let detached = {
        let runtime = app.state::<Runtime>();
        let mut store = runtime.store.lock();
        if let Some(record) = store
            .pending
            .as_ref()
            .filter(|r| r.snapshot.session_id == operation)
        {
            record.data.report.outcome(ReportOutcome::Cancelled);
        }
        store.detach(operation, None)
    };
    if detached.is_some() {
        publish(app);
    }
    drop(detached);
    clear_target(app, operation);
}
