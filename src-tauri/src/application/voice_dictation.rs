//! Explicit command dictation retains the ordinary capture and scheduler rules.
use crate::{
    application,
    operation::{OperationSource, TerminalReason},
    pipeline, stt,
    types::PipelineState,
};
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
    use crate::pipeline::completion::{wait, FinishClaim};
    use crate::{error::AppError, types::Transcript};
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
                .map(|_| Transcript {
                    text: String::new(),
                    detected_language: None,
                    transcribe_secs: None,
                    audio_secs: None,
                    device: None,
                })
                .map_err(|error| AppError::Internal(error.to_string()));
            ticket.complete(&completion);
            result
        }
    }
}

async fn run_once(
    app: &tauri::AppHandle,
    operation: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        Ok(None) => return Ok(()),
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
        return Ok(());
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
        return Ok(());
    }

    let mut gate = super::speech_gate::SpeechGate::new(app)?;
    if !gate.recording_has_speech(&samples, || cancellation.is_cancelled()) {
        pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(());
    }
    let (samples, _) = crate::vad::trim_leading_silence_with_result(&samples);
    if samples.is_empty() {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(());
    }

    // Загружаем основную whisper-модель.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        let stt = pipeline.stt().clone();
        let path = std::path::PathBuf::from(path);
        let acceleration = settings.acceleration;
        let worker_paths = stt::worker_paths_for_app(app);
        let scheduler = pipeline.scheduler();
        let load_cancel = cancellation.clone();
        let background_activity = activity.clone();
        let load = tauri::async_runtime::spawn_blocking(move || {
            let _activity = background_activity;
            let _permit = scheduler.acquire(true, &load_cancel)?;
            stt.ensure_loaded(&path, acceleration, &worker_paths)
        });
        let load_result = tokio::select! {
            result = load => Some(result),
            _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
        };
        let Some(load_result) = load_result else {
            return Ok(());
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
        return Err("Whisper-модель не выбрана".into());
    }

    if !pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Transcribing,
        TerminalReason::Completed,
    ) {
        return Ok(());
    }
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let stt_cancellation = cancellation.clone();
    let scheduler = pipeline.scheduler();
    let background_activity = activity.clone();
    let transcribe = tauri::async_runtime::spawn_blocking(move || {
        let _activity = background_activity;
        let permit = scheduler.acquire(true, &stt_cancellation)?;
        stt.transcribe_cancellable(&samples, &language, permit.cancellation.clone())
    });
    let transcript = tokio::select! {
        result = transcribe => Some(result),
        _ = application::dictation::wait_for_cancellation(cancellation) => None,
    };
    let Some(transcript) = transcript else {
        return Ok(());
    };
    let transcript = transcript.map_err(|e| format!("transcribe join: {e}"))??;

    if !pipeline.is_operation_active(operation) {
        tracing::info!(
            "voice command transcript discarded because dictation was cancelled or replaced"
        );
        return Ok(());
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
    let _ = app.emit("command-proposal", transcript.text);
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
    Ok(())
}
