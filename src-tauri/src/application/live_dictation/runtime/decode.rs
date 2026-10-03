use super::super::session::LiveSession;
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    stt::WindowTranscript,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub(super) async fn load(app: &AppHandle, session: &Arc<LiveSession>) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let path = session
        .settings
        .whisper_model_path
        .clone()
        .ok_or(AppError::ModelNotLoaded)?;
    let acceleration = session.settings.acceleration;
    let stt = app.state::<Pipeline>().stt().clone();
    let scheduler = app.state::<Pipeline>().scheduler();
    let cancellation = session.cancellation.clone();
    let workers = crate::stt::worker_paths_for_app(app);
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _permit = scheduler.acquire(true, &cancellation)?;
        stt.ensure_loaded(std::path::Path::new(&path), acceleration, &workers)
    });
    tokio::select! {
        result = task => result.map_err(|error| AppError::Internal(format!("model load: {error}")))?,
        _ = crate::application::dictation::wait_for_cancellation(session.cancellation.clone()) => Err(AppError::Cancelled("Загрузка отменена".into())),
    }
}

pub(super) async fn window(
    app: &AppHandle,
    session: &Arc<LiveSession>,
    samples: Vec<i16>,
    from: u64,
) -> AppResult<WindowTranscript> {
    let activity = crate::application::updates::activity::lease()?;
    let stt = app.state::<Pipeline>().stt().clone();
    let scheduler = app.state::<Pipeline>().scheduler();
    let language = session.settings.language.clone();
    let operation = session.operation;
    let cancellation = session.cancellation.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let permit = scheduler.acquire(true, &cancellation)?;
        stt.transcribe_window(
            &samples,
            &language,
            None,
            operation,
            &permit.cancellation,
            from,
        )
    });
    tokio::select! {
        result = task => result.map_err(|error| AppError::Internal(format!("live window: {error}")))?,
        _ = crate::application::dictation::wait_for_cancellation(session.cancellation.clone()) => Err(AppError::Cancelled("Распознавание отменено".into())),
    }
}
