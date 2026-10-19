//! Stop joins actual capture work off the async executor and decodes only its tail.
use super::{await_task, Session};
use crate::{
    application::{dictation::segment_completion, updates::activity},
    error::{AppError, AppResult},
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
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("segment completion cancelled".into()));
        }
        if batch.processed_until == samples.len() {
            return segment_completion::complete(
                operation,
                &settings,
                &samples,
                batch,
                &[],
                &cancellation,
                |_, _, _| unreachable!("no remaining PCM"),
            );
        }
        let vad_path = crate::application::speech_gate::model_path(&app)?;
        let ranges = segment_completion::tail_ranges(
            &samples,
            batch.processed_until,
            &vad_path,
            &cancellation,
        )?;
        let mut gate = fono_audio::StreamingSpeechVad::new(&vad_path)
            .map_err(|error| AppError::Stt(error.to_string()))?;
        let workers = crate::stt::worker_paths_for_app(&app);
        let model = settings
            .selected_stt_model()
            .ok_or(AppError::ModelNotSelected)?;
        segment_completion::complete(
            operation,
            &settings,
            &samples,
            batch,
            &ranges,
            &cancellation,
            |window, start, context| {
                // A final fragment can be shorter than the neural detector's
                // evidence window, especially after a forced duration cut.
                // Use the admitted whole capture rather than lose that edge.
                if window.len() < fono_audio::speech_vad::MIN_RECORDING_SAMPLES {
                    return Err(AppError::Config(
                        "short tail requires complete-capture fallback".into(),
                    ));
                }
                let speech = gate
                    .analyze_recording(window, || cancellation.is_cancelled())
                    .ok_or_else(|| {
                        AppError::Cancelled("tail speech validation cancelled".into())
                    })?;
                if !speech.has_speech {
                    return Ok(None);
                }
                let permit = scheduler.acquire(true, &cancellation)?;
                stt.ensure_model_loaded(
                    &model,
                    settings.acceleration,
                    &workers,
                    &permit.cancellation,
                )?;
                let context = context.filter(|_| {
                    stt.capabilities()
                        .is_some_and(|capabilities| capabilities.initial_prompt)
                });
                stt.transcribe_window_with_options(
                    window,
                    &settings.language,
                    context,
                    operation,
                    &permit.cancellation,
                    start,
                    settings.decoding_options(),
                )
                .map(Some)
            },
        )
    });
    await_task(task, session.cancellation.clone(), "segmented Stop").await
}
