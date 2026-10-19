//! Explicit command dictation retains the ordinary capture and scheduler rules.
use crate::application::dictation::DictationOutcome;
use crate::{application, pipeline, stt, types::PipelineState};
use fono_core::{OperationSource, TerminalReason};
use tauri::{AppHandle, Emitter, Manager};
struct CommandCleanup {
    app: AppHandle,
    operation: u64,
}
impl Drop for CommandCleanup {
    fn drop(&mut self) {
        let pipeline = self.app.state::<pipeline::Pipeline>();
        if pipeline.is_operation_active(self.operation) {
            if let Some(cancel) = pipeline.cancellation(self.operation) {
                cancel.cancel();
            }
            let _ = pipeline.stop_recording_for(self.operation);
            pipeline::set_state_for_operation(
                &self.app,
                self.app.state::<crate::state::AppState>().inner(),
                &pipeline,
                self.operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
        }
        application::dictation::resume_wake_if_idle(&self.app);
    }
}
pub(crate) async fn run(
    app: &tauri::AppHandle,
    operation: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::error::AppError;
    use crate::pipeline::completion::{wait, FinishClaim};
    let pipeline = app.state::<pipeline::Pipeline>();
    if !pipeline.is_operation_active(operation) {
        return Ok(());
    }
    match pipeline.completion.claim(operation)? {
        FinishClaim::Waiting(receiver) => {
            wait(receiver).await?;
            Ok(())
        }
        FinishClaim::Owner(ticket) => {
            let result = run_once(app, operation).await;
            let completion = result
                .as_ref()
                .cloned()
                .map_err(|error| AppError::Internal(error.to_string()));
            ticket.complete(&completion);
            if let Ok(outcome) = &completion {
                crate::events::emit_dictation_outcome(app, operation, outcome.kind());
            }
            result.map(|_| ())
        }
    }
}

async fn run_once(
    app: &tauri::AppHandle,
    operation: u64,
) -> Result<DictationOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let activity = crate::application::updates::activity::lease()?;
    let state = app.state::<crate::state::AppState>();
    let pipeline = app.state::<pipeline::Pipeline>();
    let settings = pipeline
        .session_settings_for(operation)
        .unwrap_or_else(|| state.settings());
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or("active voice command has no cancellation signal")?;

    let _cleanup = CommandCleanup {
        app: app.clone(),
        operation,
    };
    let samples = match pipeline.stop_recording_for(operation) {
        Ok(Some(samples)) => samples,
        Ok(None) => return Ok(DictationOutcome::interrupted(&pipeline, operation)),
        Err(error) => {
            let _ = pipeline::set_state_for_operation(
                app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(Box::new(error));
        }
    };
    if !pipeline.is_operation_active(operation) {
        tracing::info!("voice command discarded because dictation was cancelled or replaced");
        return Ok(DictationOutcome::interrupted(&pipeline, operation));
    }
    if samples.is_empty() {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(DictationOutcome::NoSpeech);
    }

    let (samples, speech) =
        super::speech_gate::analyze_recording(app.clone(), samples, cancellation.clone()).await?;
    if !speech.is_some_and(|decision| decision.has_speech) {
        let outcome = if speech.is_none() || !pipeline.is_operation_active(operation) {
            DictationOutcome::interrupted(&pipeline, operation)
        } else {
            DictationOutcome::NoSpeech
        };
        pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            outcome.terminal_reason(),
        );
        return Ok(outcome);
    }
    let (samples, _) = crate::vad::prepare_recording_with_result(&samples, &speech.unwrap());
    if samples.is_empty() {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(DictationOutcome::NoSpeech);
    }

    if let Some(model) = settings.selected_stt_model() {
        let stt = pipeline.stt().clone();
        let acceleration = settings.acceleration;
        let worker_paths = stt::worker_paths_for_app(app);
        let scheduler = pipeline.scheduler();
        let load_cancel = cancellation.clone();
        let background_activity = activity.clone();
        let load = tauri::async_runtime::spawn_blocking(move || {
            let _activity = background_activity;
            let _permit = scheduler.acquire(true, &load_cancel)?;
            stt.ensure_model_loaded(&model, acceleration, &worker_paths, &load_cancel)
        });
        let load_result = tokio::select! {
            result = load => Some(result),
            _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
        };
        let Some(load_result) = load_result else {
            return Ok(DictationOutcome::interrupted(&pipeline, operation));
        };
        if let Err(error) = load_result.map_err(|error| format!("model load join: {error}"))? {
            let _ = pipeline::set_state_for_operation(
                app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(Box::new(error));
        }
    } else {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Failed,
        );
        return Err("Модель распознавания не выбрана".into());
    }

    if !pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Transcribing,
        TerminalReason::Completed,
    ) {
        return Ok(DictationOutcome::interrupted(&pipeline, operation));
    }
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let decoding = settings.decoding_options();
    let stt_cancellation = cancellation.clone();
    let scheduler = pipeline.scheduler();
    let background_activity = activity.clone();
    let transcribe = tauri::async_runtime::spawn_blocking(move || {
        let _activity = background_activity;
        let permit = scheduler.acquire(true, &stt_cancellation)?;
        stt.transcribe_cancellable_with_options(
            &samples,
            &language,
            permit.cancellation.clone(),
            decoding,
        )
    });
    let transcript = tokio::select! {
        result = transcribe => Some(result),
        _ = application::dictation::wait_for_cancellation(cancellation) => None,
    };
    let Some(transcript) = transcript else {
        return Ok(DictationOutcome::interrupted(&pipeline, operation));
    };
    let transcript = transcript.map_err(|e| format!("transcribe join: {e}"))??;

    if !pipeline.is_operation_active(operation) {
        tracing::info!(
            "voice command transcript discarded because dictation was cancelled or replaced"
        );
        return Ok(DictationOutcome::interrupted(&pipeline, operation));
    }

    tracing::info!(
        "voice command transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    let proposal = application::command_proposal::create(
        state.inner(),
        operation,
        OperationSource::Hotkey,
        transcript.text.clone(),
        None,
    );
    tracing::info!(
        proposal_id = proposal.id,
        operation = proposal.operation_id,
        expires_at = %proposal.expires_at,
        "voice command proposal created"
    );
    let _ = app.emit("command-proposal", transcript.text.clone());
    if let Some(settings_window) = app.get_webview_window("settings") {
        let _ = settings_window.unminimize();
        let _ = settings_window.show();
        let _ = settings_window.set_focus();
    }
    let _ = pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );
    Ok(DictationOutcome::Deferred(transcript))
}
