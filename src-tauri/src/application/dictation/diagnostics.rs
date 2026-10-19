//! User-triggered bounded test capture owns the same STT scheduler and lifecycle.
use super::{
    capture::ensure_capture_allowed,
    lifecycle::{emit_pipeline_error, OperationScope, Session},
    postprocess, recognition, wait_for_cancellation, DictationOutcome,
};
use crate::{
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{PipelineState, Transcript},
};
use fono_core::{OperationSource, TerminalReason};
use tauri::{AppHandle, Manager};

/// Record 500–30000 ms. No archive or desktop result is published; injection is optional.
pub(crate) async fn transcribe_test(
    app: AppHandle,
    duration_ms: u64,
    inject: Option<bool>,
) -> AppResult<DictationOutcome> {
    let pipeline = app.state::<Pipeline>();
    let operation = {
        let _admission = crate::application::capture_configuration::begin_capture()?;
        ensure_capture_allowed(&app)?;
        let settings = app.state::<AppState>().settings();
        if settings.selected_stt_model().is_none() {
            let message = "Whisper model is not selected. Download and choose a model in settings.";
            emit_pipeline_error(&app, message);
            return Err(AppError::Stt(message.into()));
        }
        let operation = pipeline
            .start_recording_from(
                settings.audio_device_id.as_deref(),
                OperationSource::Diagnostics,
            )
            .map_err(|error| {
                emit_pipeline_error(&app, &error.to_string());
                error
            })?;
        if !pipeline.set_session_settings_for(operation, settings) {
            pipeline.stop_recording_for(operation)?;
            return Err(AppError::Cancelled("Проверка микрофона отменена".into()));
        }
        operation
    };
    // Pause only after successful capture acquisition. A failed concurrent start
    // must not resume the wake pause owned by another dictation.
    let mut scope = OperationScope::new(app.clone(), operation, true);
    if inject.unwrap_or(false) {
        let capture_app = app.clone();
        let activity = crate::application::updates::activity::lease()?;
        tokio::task::spawn_blocking(move || {
            let _activity = activity;
            super::workflow::capture_target(&capture_app, operation, OperationSource::Diagnostics);
        })
        .await
        .map_err(|_| AppError::Injection("diagnostic destination capture failed".into()))?;
    }
    pipeline::set_state_for_operation(
        &app,
        app.state::<AppState>().inner(),
        &pipeline,
        operation,
        PipelineState::Listening,
        TerminalReason::Completed,
    );
    let mut result = run(&app, operation, duration_ms, inject.unwrap_or(false)).await;
    scope.complete_outcome(&mut result);
    drop(scope);
    if let Ok(outcome) = &result {
        crate::events::emit_dictation_outcome(&app, operation, outcome.kind());
    }
    result
}

async fn run(
    app: &AppHandle,
    operation: u64,
    duration_ms: u64,
    inject: bool,
) -> AppResult<DictationOutcome> {
    let session = Session::current(app, operation)?;
    let duration = std::time::Duration::from_millis(duration_ms.clamp(500, 30_000));
    tokio::select! {
        _ = tokio::time::sleep(duration) => {},
        _ = wait_for_cancellation(session.cancellation.clone()) => return Ok(session.interrupted()),
    }
    if !session.active("diagnostic after recording") {
        return Ok(session.interrupted());
    }
    let samples = app
        .state::<Pipeline>()
        .stop_recording_for(operation)
        .map_err(|error| {
            session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
            error
        })?;
    let Some(samples) = samples else {
        return Ok(session.interrupted());
    };
    if samples.len() < 1600 {
        let error = AppError::Audio("записано слишком мало аудио — проверьте, что микрофон работает и не занят другим приложением".into());
        session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
        return Err(error);
    }
    let (samples, speech) = crate::application::speech_gate::analyze_recording(
        app.clone(),
        samples,
        session.cancellation.clone(),
    )
    .await?;
    let Some(speech) = speech else {
        return Ok(session.interrupted());
    };
    if !speech.has_speech {
        return Ok(
            if session.cancellation.is_cancelled()
                || !session.active("diagnostic speech validation")
            {
                session.interrupted()
            } else {
                DictationOutcome::NoSpeech
            },
        );
    }
    if !session.transition(PipelineState::Transcribing, TerminalReason::Completed) {
        return Ok(session.interrupted());
    }
    let (samples, _) = crate::vad::prepare_recording_with_result(&samples, &speech);
    let transcript = match recognition::run(&session, samples).await {
        Ok(Some(transcript)) => transcript,
        Ok(None) => return Ok(session.interrupted()),
        Err(error) => {
            session.error(crate::events::ErrorCodeV1::Stt, error.to_string());
            return Err(error);
        }
    };
    let Some(final_text) = postprocess::process(&session, &transcript, None).await else {
        return Ok(session.interrupted());
    };
    if !session.active("diagnostic before result") {
        return Ok(session.interrupted());
    }
    if inject && !final_text.is_empty() {
        if !session.transition(PipelineState::Injecting, TerminalReason::Completed) {
            return Ok(session.interrupted());
        }
        if let Err(error) = super::workflow::insert_captured(&session, &final_text).await {
            session.error(
                crate::events::ErrorCodeV1::Injection,
                format!("Вставка текста: {error}"),
            );
        }
    }
    Ok(DictationOutcome::Completed(Transcript {
        text: final_text,
        ..transcript
    }))
}
