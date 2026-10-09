//! Append-only interactive dictation. No postprocessor is called in this mode.
mod insertion;
mod runtime;
mod session;

use crate::{
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{AiMode, PipelineState, Transcript},
};
use fono_core::OperationSource;
use parking_lot::Mutex;
pub use session::LiveSnapshot;
use session::{LiveData, LiveSession};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct LiveController(Mutex<Option<Arc<LiveSession>>>);

fn current(app: &AppHandle) -> Option<Arc<LiveSession>> {
    app.state::<LiveController>().0.lock().clone()
}

fn owns(app: &AppHandle, session: &LiveSession) -> bool {
    current(app).is_some_and(|item| item.operation == session.operation)
}

pub fn snapshot(app: &AppHandle) -> Option<LiveSnapshot> {
    current(app).map(|session| session.data.lock().snapshot.clone())
}

pub fn is_active(app: &AppHandle) -> bool {
    current(app).is_some_and(|session| !session.finished.load(Ordering::Acquire))
}

pub fn start(
    app: AppHandle,
    source: OperationSource,
    pre_roll: &[i16],
    cursor: Option<(u64, u64)>,
) -> AppResult<u64> {
    if is_active(&app) {
        return Err(AppError::Busy("Живая диктовка ещё выполняется".into()));
    }
    let mut settings = app.state::<AppState>().settings();
    if settings.whisper_model_path.is_none() {
        return Err(AppError::ModelNotLoaded);
    }
    settings.ai_mode = AiMode::Off;
    let gate = super::speech_gate::SpeechGate::new(&app)?;
    // The main window can also start a preview without an external destination.
    // Binding an external field always requires a current, explicit focus.
    let target = crate::injection::target::capture().ok();
    let controller = app.state::<LiveController>();
    let mut slot = controller.0.lock();
    if slot
        .as_ref()
        .is_some_and(|session| !session.finished.load(Ordering::Acquire))
    {
        return Err(AppError::Busy("Живая диктовка ещё выполняется".into()));
    }
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.start_capture(
        settings.audio_device_id.as_deref(),
        pre_roll,
        source,
        cursor,
        true,
    )?;
    if !pipeline.set_session_settings_for(operation, settings.clone()) {
        let _ = pipeline.stop_recording_for(operation);
        return Err(AppError::Cancelled("Начало диктовки отменено".into()));
    }
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or_else(|| AppError::Internal("Нет сигнала отмены диктовки".into()))?;
    let (completion, _) = tokio::sync::watch::channel(None);
    let session = Arc::new(LiveSession {
        operation,
        settings,
        cancellation,
        started: std::time::Instant::now(),
        stop: Default::default(),
        finished: Default::default(),
        terminal_started: Default::default(),
        completion,
        insertion: tokio::sync::Mutex::new(()),
        data: Mutex::new(LiveData::new(operation, source, target)),
    });
    *slot = Some(session.clone());
    drop(slot);
    app.state::<fono_wake::WakeWordHandle>().pause();
    pipeline::set_state_for_operation(
        &app,
        app.state::<AppState>().inner(),
        &pipeline,
        operation,
        PipelineState::Listening,
        fono_core::TerminalReason::Completed,
    );
    session.emit(&app);
    tauri::async_runtime::spawn(insertion::observe_focus(app.clone(), session.clone()));
    tauri::async_runtime::spawn(runtime::run(app, session, gate));
    Ok(operation)
}

pub async fn finish(app: AppHandle) -> AppResult<Transcript> {
    let session = current(&app).ok_or_else(|| AppError::Config("Нет живой диктовки".into()))?;
    finish_session(&app, session).await
}

pub async fn finish_for(app: AppHandle, operation: u64) -> AppResult<Transcript> {
    let session = current(&app)
        .filter(|session| session.operation == operation)
        .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()))?;
    finish_session(&app, session).await
}

async fn finish_session(app: &AppHandle, session: Arc<LiveSession>) -> AppResult<Transcript> {
    if !session.finished.load(Ordering::Acquire) && !session.stop.swap(true, Ordering::AcqRel) {
        app.state::<Pipeline>()
            .stop_capture_for(session.operation)?;
        session.data.lock().snapshot.phase = "draining".into();
        session.emit(app);
    }
    crate::pipeline::completion::wait(session.completion.subscribe()).await
}

pub fn cancel(app: &AppHandle) -> AppResult<()> {
    let Some(session) = current(app).filter(|session| !session.finished.load(Ordering::Acquire))
    else {
        return Ok(());
    };
    cancel_session(app, session)
}

pub(crate) fn cancel_for(app: &AppHandle, operation: u64) -> AppResult<()> {
    let session = current(app)
        .filter(|session| session.operation == operation)
        .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()))?;
    cancel_session(app, session)
}

fn cancel_session(app: &AppHandle, session: Arc<LiveSession>) -> AppResult<()> {
    if !session.request_cancel() {
        return Ok(());
    }
    app.state::<Pipeline>()
        .stop_capture_for(session.operation)?;
    // The runtime finalizes the available partial text before releasing its slot.
    // New recordings cannot replace its buffer while it acknowledges insertion.
    Ok(())
}

pub async fn resume(app: AppHandle) -> AppResult<()> {
    let session = current(&app).ok_or_else(|| AppError::Config("Нет живой диктовки".into()))?;
    if session.finished.load(Ordering::Acquire) || session.cancellation.is_cancelled() {
        return Err(AppError::Config(
            "Диктовка уже завершена; скопируйте сохранённый текст".into(),
        ));
    }
    let _serial = session.insertion.lock().await;
    if session.stopping()
        || session.finished.load(Ordering::Acquire)
        || session.cancellation.is_cancelled()
        || !owns(&app, &session)
    {
        return Err(AppError::Config("Диктовка уже завершается".into()));
    }
    if session.data.lock().snapshot.insertion_state == "failed" {
        return Err(AppError::Injection(
            "Вставка не подтверждена. Скопируйте текст вручную, чтобы избежать дублей".into(),
        ));
    }
    let target = tauri::async_runtime::spawn_blocking(crate::injection::target::capture)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
    if session.stopping()
        || session.finished.load(Ordering::Acquire)
        || session.cancellation.is_cancelled()
        || !owns(&app, &session)
    {
        return Err(AppError::Config("Диктовка уже завершается".into()));
    }
    let mut data = session.data.lock();
    data.target = Some(target);
    data.snapshot.insertion_state = "active".into();
    data.snapshot.warning = None;
    drop(data);
    session.emit(&app);
    Ok(())
}
