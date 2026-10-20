//! Native shortcut adapter shared by initial registration and settings updates.
pub(crate) mod capture;
#[cfg(any(windows, test))]
mod gesture;
#[cfg(windows)]
mod hooks;
mod state;
pub(crate) mod validation;

#[cfg(windows)]
pub(crate) use hooks::HookRuntime;

#[cfg(not(windows))]
#[derive(Default)]
pub(crate) struct HookRuntime {}

#[cfg(not(windows))]
impl HookRuntime {
    pub fn stop(&self) {}
    fn start(
        &self,
        _: &AppHandle,
        trigger: DictationTrigger,
        _: HotkeyMode,
    ) -> Result<(), &'static str> {
        if trigger == DictationTrigger::Shortcut {
            Ok(())
        } else {
            Err("Триггеры клавиатуры и мыши доступны только в Windows.")
        }
    }
}

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState as NativeState};

use crate::{
    events,
    pipeline::Pipeline,
    types::{DictationTrigger, HotkeyMode, Settings},
};
use fono_core::OperationSource;
use state::{Action, ActiveCapture, KeyEvent, ShortcutState};

pub(crate) fn register_all_shortcuts(
    app: &AppHandle,
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    validation::validate(settings)?;
    let shortcuts = app.global_shortcut();
    let hooks = app.state::<HookRuntime>();
    hooks.stop();
    shortcuts.unregister_all()?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let dictation_state = Arc::new(Mutex::new(ShortcutState::default()));
        let dictation_mode = settings.hotkey_mode;
        if settings.dictation_trigger == DictationTrigger::Shortcut {
            shortcuts.on_shortcut(settings.hotkey.as_str(), move |app, _, event| {
                handle(
                    app,
                    &dictation_state,
                    key_event(event.state),
                    dictation_mode,
                    false,
                );
            })?;
        }

        let command_state = Arc::new(Mutex::new(ShortcutState::default()));
        shortcuts.on_shortcut(settings.command_hotkey.as_str(), move |app, _, event| {
            handle(
                app,
                &command_state,
                key_event(event.state),
                HotkeyMode::Hold,
                true,
            );
        })?;
        hooks.start(app, settings.dictation_trigger, dictation_mode)?;
        if settings.selection_capture_enabled {
            shortcuts.on_shortcut(settings.selection_hotkey.as_str(), |app, _, event| {
                if event.state == NativeState::Released {
                    crate::application::selection_capture::request(app.clone());
                }
            })?;
        }
        tracing::info!("dictation and voice command triggers registered");
        Ok(())
    })();
    if result.is_err() {
        hooks.stop();
        let _ = shortcuts.unregister_all();
    }
    result
}

fn key_event(event: NativeState) -> KeyEvent {
    match event {
        NativeState::Pressed => KeyEvent::Pressed,
        NativeState::Released => KeyEvent::Released,
    }
}

fn active_capture(app: &AppHandle) -> Option<ActiveCapture> {
    let pipeline = app.state::<Pipeline>();
    pipeline.current_operation().map(|operation| ActiveCapture {
        operation: operation.id,
        recording: pipeline.is_recording(),
    })
}

fn handle(
    app: &AppHandle,
    state: &Arc<Mutex<ShortcutState>>,
    event: KeyEvent,
    mode: HotkeyMode,
    command: bool,
) {
    // Idle hooks do not block updates. Actual capture preparation and queued
    // Stop/flush work hold admission until the real work has finished.
    let Ok(activity) = super::updates::activity::lease() else {
        return;
    };
    // Hold the key state through synchronous capture startup. A quick release
    // cannot race ahead of storing the newly allocated operation identifier.
    let mut key = state.lock();
    let action = key.event(event, mode, active_capture(app));
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
            let _activity = activity;
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
