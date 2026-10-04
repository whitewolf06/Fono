//! Wake only decides when recording starts and ends. Recognition, publication
//! and persistence belong to the common dictation use case.
use super::speech_gate::SpeechGate;
use crate::{
    error::AppResult,
    operation::{OperationSource, TerminalReason},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{DictationMode, PipelineState},
};
use tauri::{AppHandle, Emitter, Manager};

pub async fn run(
    app: &AppHandle,
    pre_roll: Vec<i16>,
    cursor: Option<fono_wake::AudioCursor>,
) -> AppResult<()> {
    crate::types::ensure_wake_available()?;
    let settings = app.state::<AppState>().settings();
    let mut vad = SpeechGate::new(app)?;
    let operation = super::dictation::start_with_audio_operation(
        app.clone(),
        OperationSource::WakeWord,
        &pre_roll,
        cursor.map(|c| (c.epoch, c.sample)),
    )?;
    if settings.dictation_mode == DictationMode::Live {
        return Ok(());
    }
    let pipeline = app.state::<Pipeline>();
    let started = std::time::Instant::now();
    let mut consumed = 0;
    let mut last_audio = std::time::Instant::now();
    let silence_ms = settings.wake_dictation_silence_ms.clamp(500, 10_000);
    loop {
        if !pipeline.is_operation_active(operation) || !pipeline.is_recording() {
            return Ok(());
        }
        let (offset, pcm) = pipeline.recording_window(consumed, 16_000);
        if !pcm.is_empty() {
            last_audio = std::time::Instant::now();
        }
        if let Some(error) = app.state::<fono_wake::AudioHub>().diagnostics().last_error {
            let _ = super::dictation::stop_with_reason_for(
                app.clone(),
                operation,
                super::dictation_tail_diagnostics::DictationStopReason::WakeTimeout,
            )
            .await;
            return Err(crate::error::AppError::Audio(format!(
                "Микрофон отключён: {error}"
            )));
        }
        if last_audio.elapsed().as_secs() >= 3 {
            let _ = super::dictation::stop_with_reason_for(
                app.clone(),
                operation,
                super::dictation_tail_diagnostics::DictationStopReason::WakeTimeout,
            )
            .await;
            return Err(crate::error::AppError::Audio(
                "Микрофон перестал передавать звук".into(),
            ));
        }
        consumed = offset + pcm.len() as u64;
        let decision = vad.accept(&pcm);
        let silent_ms = decision.processed.saturating_sub(decision.last_speech) * 1000 / 16_000;
        crate::events::emit_wake_countdown(
            app,
            crate::events::WakeCountdownV1 {
                remaining_ms: silence_ms.saturating_sub(silent_ms),
                timeout_ms: silence_ms,
                speaking: decision.speaking,
            },
        );
        if !decision.has_speech && started.elapsed().as_secs() >= 5 {
            if let Some(cancellation) = pipeline.cancellation(operation) {
                cancellation.cancel();
            }
            let _ = pipeline.stop_recording_for(operation);
            pipeline::set_state_for_operation(
                app,
                app.state::<AppState>().inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Cancelled,
            );
            super::dictation::resume_wake_if_idle(app);
            return Ok(());
        }
        if pipeline.is_operation_confirmed(operation)
            || (decision.has_speech && silent_ms >= silence_ms)
            || started.elapsed().as_secs() >= pipeline::MAX_RECORDING_SECONDS as u64
            || pipeline.recording_limit_reached()
        {
            let reason = if pipeline.is_operation_confirmed(operation) {
                super::dictation_tail_diagnostics::DictationStopReason::WakeConfirmed
            } else if decision.has_speech && silent_ms >= silence_ms {
                super::dictation_tail_diagnostics::DictationStopReason::WakeSilence
            } else {
                super::dictation_tail_diagnostics::DictationStopReason::WakeTimeout
            };
            super::dictation::stop_with_reason_for(app.clone(), operation, reason).await?;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

pub fn propose_command(app: &AppHandle, operation: u64, command: String) {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    if !pipeline.is_operation_active(operation) {
        return;
    }
    if !pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Processing,
        TerminalReason::Completed,
    ) {
        return;
    }
    let proposal = super::command_proposal::create(
        state.inner(),
        operation,
        OperationSource::WakeWord,
        command,
        None,
    );
    let _ = app.emit("command-proposal", proposal.original_text);
    pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
