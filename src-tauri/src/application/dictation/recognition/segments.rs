//! Stop joins actual capture work off the async executor and decodes only its tail.
use super::{await_task, Session};
use crate::{
    application::{dictation::segment_completion, updates::activity},
    error::AppResult,
    pipeline::{capture_segments::CaptureSegmentBatch, Pipeline},
    types::Transcript,
};
use std::sync::Arc;
use tauri::Manager;

pub(super) async fn collect(session: &Session) -> AppResult<Option<CaptureSegmentBatch>> {
    // Admission precedes the pipeline capture fence; the join owns this lease
    // even after the IPC waiter or the complete Stop future is dropped.
    let activity = activity::lease()?;
    let Some(task) = session
        .app
        .state::<Pipeline>()
        .take_capture_segments_for(session.operation)
    else {
        return Ok(None);
    };
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        task.finish()
    });
    match await_task(
        task,
        session.cancellation.clone(),
        "capture segment completion",
    )
    .await
    {
        Ok(result) => Ok(result),
        Err(_)
            if session.cancellation.is_cancelled()
                || !session.active("capture segment join cancelled") =>
        {
            Ok(None)
        }
        Err(_) => {
            tracing::warn!(
                operation = session.operation,
                "capture segment worker failed; using complete recording"
            );
            Ok(None)
        }
    }
}

pub(super) async fn run(
    session: &Session,
    samples: Arc<Vec<i16>>,
    batch: CaptureSegmentBatch,
) -> AppResult<Option<Transcript>> {
    let settings = session.settings.clone();
    let operation = session.operation;
    let cancellation = session.cancellation.clone();
    let app = session.app.clone();
    let stt = app.state::<Pipeline>().stt().clone();
    let scheduler = app.state::<Pipeline>().scheduler();
    let activity = activity::lease()?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        segment_completion::Runtime {
            stt: &stt,
            scheduler: &scheduler,
        }
        .finish(operation, &settings, &samples, batch, &cancellation, || {
            Ok((
                crate::application::speech_gate::model_path(&app)?,
                crate::stt::worker_paths_for_app(&app),
            ))
        })
    });
    await_task(task, session.cancellation.clone(), "segmented Stop").await
}
