//! Every model load and inference obtains the shared interactive STT permit.
use super::{lifecycle::Session, wait_for_cancellation};
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    types::Transcript,
};
use fono_core::OperationCancellation;
use tauri::Manager;

async fn await_task<T: Send + 'static>(
    task: tauri::async_runtime::JoinHandle<AppResult<T>>,
    cancellation: OperationCancellation,
    label: &str,
) -> AppResult<Option<T>> {
    tokio::select! {
        result = task => {
            if cancellation.is_cancelled() { return Ok(None); }
            result.map_err(|error| AppError::Internal(format!("{label} join: {error}")))?.map(Some)
        },
        _ = wait_for_cancellation(cancellation.clone()) => Ok(None),
    }
}

async fn load(session: &Session) -> AppResult<bool> {
    let path = session
        .settings
        .whisper_model_path
        .as_deref()
        .ok_or_else(|| {
            AppError::Stt(
                "Whisper model is not selected. Download and choose a model in settings.".into(),
            )
        })?;
    let path = std::path::PathBuf::from(path);
    let pipeline = session.app.state::<Pipeline>();
    let stt = pipeline.stt().clone();
    let scheduler = pipeline.scheduler();
    let acceleration = session.settings.acceleration;
    let workers = crate::stt::worker_paths_for_app(&session.app);
    let cancellation = session.cancellation.clone();
    let activity = crate::application::updates::activity::lease()?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _permit = scheduler.acquire(true, &cancellation)?;
        stt.ensure_loaded(&path, acceleration, &workers)
    });
    Ok(await_task(task, session.cancellation.clone(), "model load")
        .await?
        .is_some())
}

pub(super) async fn run(session: &Session, samples: Vec<i16>) -> AppResult<Option<Transcript>> {
    run_measured(session, samples, None).await
}

pub(super) async fn run_measured(
    session: &Session,
    samples: Vec<i16>,
    report: Option<&crate::application::diagnostic_report::telemetry::ReportTrace>,
) -> AppResult<Option<Transcript>> {
    use crate::application::diagnostic_report::telemetry::ReportStage;
    let loaded = {
        let _timer = report.map(|trace| trace.stage(ReportStage::ModelLoad));
        load(session).await?
    };
    if !loaded || !session.active("after model load") {
        return Ok(None);
    }
    let _timer = report.map(|trace| trace.stage(ReportStage::Recognition));
    let pipeline = session.app.state::<Pipeline>();
    let stt = pipeline.stt().clone();
    let scheduler = pipeline.scheduler();
    let language = session.settings.language.clone();
    let cancellation = session.cancellation.clone();
    let activity = crate::application::updates::activity::lease()?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let permit = scheduler.acquire(true, &cancellation)?;
        stt.transcribe_cancellable(&samples, &language, permit.cancellation.clone())
    });
    let result = await_task(task, session.cancellation.clone(), "transcribe").await?;
    if !session.active("after transcription") {
        return Ok(None);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::scheduler::SttScheduler;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn load_failure_propagates_to_the_terminal_cleanup_owner() {
        let task = tauri::async_runtime::spawn_blocking(|| {
            Err::<(), _>(AppError::Stt("model load failed".into()))
        });
        let result = await_task(task, OperationCancellation::default(), "model load").await;
        assert!(matches!(result, Err(AppError::Stt(message)) if message == "model load failed"));
    }

    #[tokio::test]
    async fn cancelled_wait_does_not_release_a_still_running_model_load_permit() {
        let scheduler = Arc::new(SttScheduler::default());
        let loader_scheduler = scheduler.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tauri::async_runtime::spawn_blocking(move || {
            let _permit = loader_scheduler.acquire(true, &OperationCancellation::default())?;
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            Ok::<_, AppError>(())
        });
        started_rx.await.unwrap();
        let cancellation = OperationCancellation::default();
        cancellation.cancel();
        let result = await_task(task, cancellation, "model load").await;
        let entered = Arc::new(AtomicBool::new(false));
        let next_entered = entered.clone();
        let next = tauri::async_runtime::spawn_blocking(move || {
            let _permit = scheduler.acquire(true, &OperationCancellation::default())?;
            next_entered.store(true, Ordering::Release);
            Ok::<_, AppError>(())
        });
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let entered_before_release = entered.load(Ordering::Acquire);
        release_tx.send(()).unwrap();
        next.await.unwrap().unwrap();
        assert!(result.unwrap().is_none());
        assert!(!entered_before_release);
        assert!(entered.load(Ordering::Acquire));
    }
}
