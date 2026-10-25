//! Wake only decides when recording starts and ends. Recognition, publication
//! and persistence belong to the common dictation use case.
use super::speech_gate::SpeechGate;
use crate::{
    error::AppResult,
    pipeline::{self, Pipeline},
    state::AppState,
    types::PipelineState,
};
#[cfg(feature = "wake-word")]
use fono_core::OperationSource;
use fono_core::TerminalReason;
#[cfg(feature = "wake-word")]
use tauri::Emitter;
use tauri::{AppHandle, Manager};

#[cfg(feature = "wake-word")]
pub async fn run(
    app: &AppHandle,
    pre_roll: Vec<i16>,
    cursor: Option<fono_audio::AudioCursor>,
) -> AppResult<()> {
    crate::types::ensure_wake_available()?;
    let settings = app.state::<AppState>().settings();
    let vad = SpeechGate::new(app)?;
    let operation = super::dictation::start_with_audio_operation(
        app.clone(),
        OperationSource::WakeWord,
        &pre_roll,
        cursor.map(|c| (c.epoch, c.sample)),
    )?;
    if settings.dictation_mode == crate::types::DictationMode::Live {
        return Ok(());
    }
    finish(app, operation, vad, settings).await
}

#[cfg(all(windows, debug_assertions))]
pub(crate) async fn run_lira(
    app: AppHandle,
    control: std::sync::Arc<super::wake_lira::Control>,
    pre_roll: Vec<i16>,
    cursor: fono_audio::AudioCursor,
    triggered_at: std::time::Instant,
) -> AppResult<()> {
    let capture = {
        let _route = control.route.lock();
        if control.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(crate::error::AppError::Cancelled(
                "Пробуждение выключено".into(),
            ));
        }
        let capture = super::dictation::reserve_lira_operation(&app)?;
        control
            .operation
            .store(capture.operation, std::sync::atomic::Ordering::Release);
        capture
    };
    let operation = capture.operation;
    tracing::info!(
        event = "lira_handoff_reserved",
        operation,
        elapsed_ms = triggered_at.elapsed().as_millis() as u64,
        "live wake dictation reserved"
    );
    let activity = super::updates::activity::cleanup_lease();
    let handle = app.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        // Both disk/native model setup and microphone handoff stay off the IPC executor.
        let model_started = std::time::Instant::now();
        let vad = SpeechGate::new(&handle)?;
        let vad_init_ms = model_started.elapsed().as_millis() as u64;
        super::dictation::start_lira_reserved(handle.clone(), capture, &pre_roll, cursor)?;
        tracing::info!(
            event = "lira_handoff_ready",
            operation,
            elapsed_ms = triggered_at.elapsed().as_millis() as u64,
            vad_init_ms,
            "live wake dictation activated"
        );
        // Capture is already attached and the target is resolved. A short native
        // cue acknowledges readiness without waiting for overlay rendering; PCM
        // continues to accumulate while this blocking worker plays the sound.
        super::recording_sounds::wake_ready(&handle, operation, triggered_at);
        Ok::<_, crate::error::AppError>(vad)
    })
    .await
    .unwrap_or_else(|_| {
        Err(crate::error::AppError::Internal(
            "Не удалось начать диктовку".into(),
        ))
    });
    let result = match prepared {
        Ok(vad) => finish(&app, operation, vad, app.state::<AppState>().settings()).await,
        Err(error) => {
            app.state::<Pipeline>().abort_capture_setup(operation);
            Err(error)
        }
    };
    let _ = control.operation.compare_exchange(
        operation,
        0,
        std::sync::atomic::Ordering::AcqRel,
        std::sync::atomic::Ordering::Acquire,
    );
    result
}

async fn finish(
    app: &AppHandle,
    operation: u64,
    mut vad: SpeechGate,
    settings: crate::types::Settings,
) -> AppResult<()> {
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
        if let Some(error) = app.state::<fono_audio::AudioHub>().diagnostics().last_error {
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

#[cfg(feature = "wake-word")]
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
    // The ordinary Stop owner atomically settles this operation after proposal
    // creation, so Cancel and the published outcome share the same claim.
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
