//! A bounded VAD consumer owns its activity until actual native work finishes.
use crate::{
    application::{speech_gate::SpeechGate, updates::activity},
    error::AppResult,
    pipeline::{
        auto_stop::{self, Phase, SilenceClock, Status},
        Pipeline,
    },
    types::Settings,
};
use tauri::{AppHandle, Manager};

pub(super) fn arm(app: AppHandle, operation: u64, command: bool) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    let Some(current) = pipeline.current_operation().filter(|o| o.id == operation) else {
        return Ok(());
    };
    let settings = pipeline
        .session_settings_for(operation)
        .unwrap_or_else(Settings::default);
    let Some(timeout) = auto_stop::timeout(&settings, current.source, command) else {
        return Ok(());
    };
    let Some(writer) = pipeline.auto_stop_reader_for(operation) else {
        return Ok(());
    };
    let Some(cancellation) = pipeline.cancellation(operation) else {
        return Ok(());
    };
    // The coordinator and blocking consumer each retain admission. Dropping a
    // waiter cannot release the worker's lease while a native VAD call runs.
    let pending_activity = activity::lease()?;
    let worker_activity = activity::lease()?;
    pipeline.update_auto_stop(Status::new(operation, timeout));
    let worker_app = app.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _activity = worker_activity;
        if cancellation.is_cancelled()
            || !worker_app
                .state::<Pipeline>()
                .while_editable_recording(operation, || {})
        {
            return Ok(false);
        }
        let mut gate = SpeechGate::new(&worker_app)?;
        let mut clock = SilenceClock::new(operation, timeout);
        let mut cursor = 0;
        loop {
            let pipeline = worker_app.state::<Pipeline>();
            if cancellation.is_cancelled() || !pipeline.while_editable_recording(operation, || {}) {
                return Ok(false);
            }
            let (start, samples) = writer.lock().window(cursor, 512);
            if cancellation.is_cancelled() || !pipeline.while_editable_recording(operation, || {}) {
                return Ok(false);
            }
            if start != cursor {
                return Err(crate::error::AppError::Audio(
                    "auto-stop capture cursor lost".into(),
                ));
            }
            if samples.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
            cursor += samples.len() as u64;
            let status = clock.observe(&gate.accept(&samples));
            if !pipeline.update_auto_stop(status) {
                return Ok(false);
            }
            if status.phase == Phase::Stopping {
                return Ok(true);
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let _activity = pending_activity;
        match work.await {
            Ok(Ok(true)) => {
                if let Err(error) = super::standard::stop_after_overlay_flush_for(
                    app.clone(), operation,
                    crate::application::dictation_tail_diagnostics::DictationStopReason::ToggleSilence,
                ).await {
                    tracing::warn!(operation, "toggle auto-stop did not complete");
                    let pipeline = app.state::<Pipeline>();
                    if pipeline.update_auto_stop(Status { phase: Phase::Unavailable, ..Status::new(operation, timeout) }) {
                        crate::events::emit_error(&app, crate::events::ErrorCodeV1::Internal, error.to_string(), Some(operation));
                    }
                }
            }
            Ok(Ok(false)) => {}
            _ => {
                let pipeline = app.state::<Pipeline>();
                if pipeline.update_auto_stop(Status {
                    phase: Phase::Unavailable,
                    ..Status::new(operation, timeout)
                }) {
                    crate::events::emit_error(
                        &app,
                        crate::events::ErrorCodeV1::Audio,
                        "Автостоп по тишине недоступен. Завершите запись вручную.",
                        Some(operation),
                    );
                }
            }
        }
    });
    Ok(())
}
