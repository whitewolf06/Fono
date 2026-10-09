//! User-triggered bounded test capture owns the same STT scheduler and lifecycle.
use super::{
    capture::ensure_capture_allowed,
    empty_transcript,
    lifecycle::{emit_pipeline_error, OperationScope, Session},
    postprocess, recognition, wait_for_cancellation,
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
) -> AppResult<Transcript> {
    let pipeline = app.state::<Pipeline>();
    let operation = {
        let _admission = crate::application::capture_configuration::begin_capture()?;
        ensure_capture_allowed(&app)?;
        let settings = app.state::<AppState>().settings();
        if settings.whisper_model_path.is_none() {
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
    pipeline::set_state_for_operation(
        &app,
        app.state::<AppState>().inner(),
        &pipeline,
        operation,
        PipelineState::Listening,
        TerminalReason::Completed,
    );
    let result = run(&app, operation, duration_ms, inject.unwrap_or(false)).await;
    scope.complete(&result);
    result
}

async fn run(
    app: &AppHandle,
    operation: u64,
    duration_ms: u64,
    inject: bool,
) -> AppResult<Transcript> {
    let session = Session::current(app, operation)?;
    let duration = std::time::Duration::from_millis(duration_ms.clamp(500, 30_000));
    tokio::select! {
        _ = tokio::time::sleep(duration) => {},
        _ = wait_for_cancellation(session.cancellation.clone()) => return Ok(empty_transcript()),
    }
    if !session.active("diagnostic after recording") {
        return Ok(empty_transcript());
    }
    let samples = app
        .state::<Pipeline>()
        .stop_recording_for(operation)
        .map_err(|error| {
            session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
            error
        })?;
    let Some(samples) = samples else {
        return Ok(empty_transcript());
    };
    if samples.len() < 1600 {
        let error = AppError::Audio("записано слишком мало аудио — проверьте, что микрофон работает и не занят другим приложением".into());
        session.error(crate::events::ErrorCodeV1::Audio, error.to_string());
        return Err(error);
    }
    let mut speech_gate = crate::application::speech_gate::SpeechGate::new(app)?;
    if !speech_gate.recording_has_speech(&samples, || session.cancellation.is_cancelled()) {
        return Ok(empty_transcript());
    }
    if !session.transition(PipelineState::Transcribing, TerminalReason::Completed) {
        return Ok(empty_transcript());
    }
    let transcript = match recognition::run(&session, samples).await {
        Ok(Some(transcript)) => transcript,
        Ok(None) => return Ok(empty_transcript()),
        Err(error) => {
            session.error(crate::events::ErrorCodeV1::Stt, error.to_string());
            return Err(error);
        }
    };
    let Some(final_text) = postprocess::process(&session, &transcript, None).await else {
        return Ok(empty_transcript());
    };
    if !session.active("diagnostic before result") {
        return Ok(empty_transcript());
    }
    if inject && !final_text.is_empty() {
        if !session.transition(PipelineState::Injecting, TerminalReason::Completed) {
            return Ok(empty_transcript());
        }
        if let Err(error) =
            crate::injection::inject_text(&final_text, session.settings.injection_mode)
        {
            session.error(
                crate::events::ErrorCodeV1::Injection,
                format!("Вставка текста: {error}"),
            );
        }
    }
    Ok(Transcript {
        text: final_text,
        ..transcript
    })
}
