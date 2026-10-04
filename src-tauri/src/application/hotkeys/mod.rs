//! Native shortcut adapter shared by initial registration and settings updates.
mod state;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState as NativeState};

use crate::{
    events,
    operation::OperationSource,
    pipeline::Pipeline,
    types::{HotkeyMode, Settings},
};
use state::{Action, ActiveCapture, KeyEvent, ShortcutState};

pub(crate) fn register_all_shortcuts(
    app: &AppHandle,
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;
    let dictation_state = Arc::new(Mutex::new(ShortcutState::default()));
    let dictation_mode = settings.hotkey_mode;
    shortcuts.on_shortcut(settings.hotkey.as_str(), move |app, _, event| {
        handle(app, &dictation_state, event.state, dictation_mode, false);
    })?;
    tracing::info!(hotkey = %settings.hotkey, mode = ?dictation_mode, "dictation shortcut registered");

    let command_state = Arc::new(Mutex::new(ShortcutState::default()));
    shortcuts.on_shortcut(settings.command_hotkey.as_str(), move |app, _, event| {
        handle(app, &command_state, event.state, HotkeyMode::Hold, true);
    })?;
    tracing::info!(hotkey = %settings.command_hotkey, "voice command shortcut registered");
    Ok(())
}

fn handle(
    app: &AppHandle,
    state: &Arc<Mutex<ShortcutState>>,
    event: NativeState,
    mode: HotkeyMode,
    command: bool,
) {
    // Hold the key state through synchronous capture startup. A quick release
    // cannot race ahead of storing the newly allocated operation identifier.
    let mut key = state.lock();
    let pipeline = app.state::<Pipeline>();
    let active = pipeline.current_operation().map(|operation| ActiveCapture {
        operation: operation.id,
        recording: pipeline.is_recording(),
    });
    let action = key.event(
        match event {
            NativeState::Pressed => KeyEvent::Pressed,
            NativeState::Released => KeyEvent::Released,
        },
        mode,
        active,
    );
    if action == Action::Start {
        let started = if command {
            super::dictation::start_command_operation(app.clone())
        } else {
            super::dictation::start_operation(app.clone(), OperationSource::Hotkey)
        };
        match started {
            Ok(operation) => {
                key.started(operation);
                events::emit_pipeline_mode(
                    app,
                    if command {
                        events::PipelineModeV1::Command
                    } else {
                        events::PipelineModeV1::Dictation
                    },
                );
            }
            Err(error) => {
                events::emit_error(app, events::ErrorCodeV1::Audio, error.to_string(), None);
                tracing::warn!(%error, command, "shortcut capture failed");
            }
        }
    }
    drop(key);
    if let Action::Stop(operation) = action {
        let app = app.clone();
        let key_state = Arc::clone(state);
        tauri::async_runtime::spawn(async move {
            if command {
                if let Err(error) = super::voice_dictation::run(&app, operation).await {
                    tracing::warn!(%error, "voice command failed");
                    events::emit_error(
                        &app,
                        events::ErrorCodeV1::Internal,
                        error.to_string(),
                        None,
                    );
                }
            } else {
                if let Err(error) =
                    super::dictation::workflow::flush_overlay_processing(&app, operation).await
                {
                    {
                        let mut key = key_state.lock();
                        app.state::<Pipeline>()
                            .while_editable_recording(operation, || {
                                key.restore_stop(
                                    operation,
                                    Some(ActiveCapture {
                                        operation,
                                        recording: true,
                                    }),
                                );
                            });
                    }
                    events::emit_error(
                        &app,
                        events::ErrorCodeV1::Internal,
                        error.to_string(),
                        Some(operation),
                    );
                    if let Some(window) = app.get_webview_window("overlay") {
                        use tauri::Emitter;
                        let _ = window.emit("overlay-processing-flush-error", error.to_string());
                    }
                    return;
                }
                if let Err(error) = super::dictation::stop_with_reason_for(
                    app,
                    operation,
                    super::dictation_tail_diagnostics::DictationStopReason::Manual,
                )
                .await
                {
                    tracing::warn!(%error, "shortcut dictation stop failed");
                }
            }
        });
    }
}
