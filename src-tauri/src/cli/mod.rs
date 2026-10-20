//! Native CLI adapter. Arguments/cwd/text never enter logs or persisted state.
mod parse;
mod queue;

use crate::{
    application::{dictation, updates::activity},
    error::{AppError, AppResult},
    pipeline::Pipeline,
};
use fono_core::{OperationPhase, OperationSnapshot, TerminalReason};
use parking_lot::Mutex;
pub(crate) use parse::Command;
use std::{collections::HashSet, sync::Arc, thread::JoinHandle};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    None,
    Start,
    Stop(u64),
    Cancel(u64),
    Show,
}

fn plan(command: Command, current: Option<&OperationSnapshot>) -> AppResult<Plan> {
    // Prompt/microphone tests have a separate completion contract: CLI Stop
    // must never turn diagnostic audio into a normal archived/inserted dictation.
    if matches!(command, Command::Start | Command::Stop | Command::Toggle)
        && current.is_some_and(|op| op.source == fono_core::OperationSource::Diagnostics)
    {
        return Err(AppError::Busy(
            "Сначала завершите пробную диктовку или проверку микрофона.".into(),
        ));
    }
    match command {
        Command::Show => Ok(Plan::Show),
        Command::Cancel => Ok(current.map_or(Plan::None, |op| Plan::Cancel(op.id))),
        Command::Stop => Ok(current
            .filter(|op| op.phase == OperationPhase::Recording)
            .map_or(Plan::None, |op| Plan::Stop(op.id))),
        Command::Start | Command::Toggle => match current {
            None => Ok(Plan::Start),
            Some(op) if op.phase == OperationPhase::Recording => {
                Ok(if command == Command::Toggle {
                    Plan::Stop(op.id)
                } else {
                    Plan::None
                })
            }
            Some(_) => Err(AppError::Busy("Сначала завершите текущую диктовку.".into())),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Work {
    Stop,
    Cancel,
}

type Claims = Arc<Mutex<HashSet<(Work, u64)>>>;
struct Claim {
    claims: Claims,
    key: (Work, u64),
}
impl Claim {
    fn take(claims: &Claims, work: Work, operation: u64) -> Option<Self> {
        let key = (work, operation);
        claims.lock().insert(key).then(|| Self {
            claims: claims.clone(),
            key,
        })
    }
}
impl Drop for Claim {
    fn drop(&mut self) {
        self.claims.lock().remove(&self.key);
    }
}

#[derive(Default)]
pub(crate) struct Runtime {
    queue: Arc<queue::Queue>,
    worker: Mutex<Option<JoinHandle<()>>>,
    claims: Claims,
}

impl Runtime {
    pub fn start(&self) -> Result<(), std::io::Error> {
        let queue = self.queue.clone();
        let worker = match std::thread::Builder::new()
            .name("fono-cli".into())
            .spawn(move || queue.run())
        {
            Ok(worker) => worker,
            Err(error) => {
                self.queue.close();
                return Err(error);
            }
        };
        *self.worker.lock() = Some(worker);
        Ok(())
    }

    pub fn shutdown(&self) {
        self.queue.close();
        if let Some(worker) = self.worker.lock().take() {
            // Start may be waiting for a main-thread window dispatch. Never
            // join it on that same thread; retain real-work admission until join.
            let activity = activity::cleanup_lease();
            tauri::async_runtime::spawn_blocking(move || {
                let _activity = activity;
                let _ = worker.join();
            });
        }
    }
}

pub(crate) fn launch_command() -> Result<Command, &'static str> {
    parse::parse(std::env::args_os().skip(1))
}

pub(crate) fn argument_error(message: &str) {
    #[cfg(windows)]
    unsafe {
        // Attach to an invoking terminal only; never create a console window.
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    eprintln!("{message}");
}

pub(crate) fn configure_launch<R: tauri::Runtime>(
    context: &mut tauri::Context<R>,
    command: Command,
) {
    configure_windows(&mut context.config_mut().app.windows, command);
}

fn configure_windows(windows: &mut [tauri::utils::config::WindowConfig], command: Command) {
    if command != Command::Show {
        if let Some(window) = windows.iter_mut().find(|window| window.label == "settings") {
            window.visible = false;
            window.focus = false;
        }
    }
}

pub(crate) fn initialize(app: &AppHandle, command: Command) -> AppResult<()> {
    app.state::<Runtime>().start()?;
    // Queue the first command before releasing the single-instance startup gate.
    // Ready is signalled by App::run, after all windows/services are constructed.
    submit(app, command)
}

pub(crate) fn ready(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<Runtime>() {
        runtime.queue.ready();
    }
}

pub(crate) fn forwarded(app: &AppHandle, arguments: Vec<String>) {
    let result = parse::parse(arguments.into_iter().skip(1))
        .map_err(|message| AppError::Config(message.into()))
        .and_then(|command| submit(app, command));
    if let Err(error) = result {
        report(app, &error, None);
    }
}

fn report(app: &AppHandle, error: &AppError, operation: Option<u64>) {
    if !matches!(error, AppError::Cancelled(_)) {
        crate::events::emit_error(
            app,
            crate::events::ErrorCodeV1::Internal,
            error.to_string(),
            operation,
        );
    }
}

fn submit(app: &AppHandle, command: Command) -> AppResult<()> {
    let runtime = app
        .try_state::<Runtime>()
        .ok_or_else(|| AppError::Busy("Fono ещё запускается. Повторите команду.".into()))?;
    if runtime.queue.closed() {
        return Err(AppError::Busy(
            "Fono завершается. Повторите команду после запуска.".into(),
        ));
    }
    let pipeline = app.state::<Pipeline>();
    let current = pipeline.current_operation();
    match plan(command, current.as_ref())? {
        Plan::None => Ok(()),
        Plan::Show => {
            let app = app.clone();
            let queue = runtime.queue.clone();
            runtime
                .queue
                .push(Box::new(move || {
                    let show = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if queue.closed() {
                            return;
                        }
                        if let Some(window) = show.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    });
                }))
                .map_err(|message| AppError::Busy(message.into()))
        }
        Plan::Start => {
            let activity = activity::lease()?;
            let capture = dictation::reserve_operation(app)?;
            let operation = capture.operation;
            let start_app = app.clone();
            let queued = runtime.queue.push(Box::new(move || {
                let _activity = activity;
                if let Err(error) = dictation::start_reserved_operation(start_app.clone(), capture)
                {
                    // Only this reserved identity may be retired after an old job fails.
                    if let Some(event) = start_app.state::<Pipeline>().cancel_for(operation) {
                        crate::events::emit_operation(&start_app, event);
                    }
                    report(&start_app, &error, Some(operation));
                } else {
                    crate::events::emit_pipeline_mode(
                        &start_app,
                        crate::events::PipelineModeV1::Dictation,
                    );
                }
            }));
            if let Err(message) = queued {
                if let Some(event) = pipeline.finish_operation(operation, TerminalReason::Failed) {
                    crate::events::emit_operation(app, event);
                }
                return Err(AppError::Busy(message.into()));
            }
            Ok(())
        }
        Plan::Stop(operation) => {
            let Some(claim) = Claim::take(&runtime.claims, Work::Stop, operation) else {
                return Ok(());
            };
            let activity = activity::lease()?;
            let app = app.clone();
            runtime
                .queue
                .push(Box::new(move || {
                    tauri::async_runtime::spawn(async move {
                        let _activity = activity;
                        let _claim = claim;
                        let result = stop(&app, operation).await;
                        if let Err(error) = result {
                            report(&app, &error, Some(operation));
                        }
                    });
                }))
                .map_err(|message| AppError::Busy(message.into()))
        }
        Plan::Cancel(operation) => {
            let Some(claim) = Claim::take(&runtime.claims, Work::Cancel, operation) else {
                return Ok(());
            };
            let activity = activity::lease()?;
            let app = app.clone();
            // Cancel bypasses queued Start/Stop and never waits for recognition.
            // Native teardown may take the capture lock, so keep it off the UI thread.
            tauri::async_runtime::spawn_blocking(move || {
                let _activity = activity;
                let _claim = claim;
                let result = tauri::async_runtime::block_on(
                    crate::ipc::dictation::cancel_dictation(app.clone(), Some(operation)),
                );
                if let Err(error) = result {
                    report(&app, &error, Some(operation));
                }
            });
            Ok(())
        }
    }
}

async fn stop(app: &AppHandle, operation: u64) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    if !pipeline.is_operation_active(operation) {
        return Ok(());
    }
    if pipeline.session_settings_for(operation).is_none() {
        return Err(AppError::Busy(
            "Запись ещё запускается. Повторите завершение.".into(),
        ));
    }
    if pipeline.is_command_capture(operation) {
        return crate::application::voice_dictation::run(app, operation)
            .await
            .map_err(|error| AppError::Internal(error.to_string()));
    }
    dictation::workflow::flush_overlay_processing(app, operation).await?;
    dictation::stop_with_reason_for(
        app.clone(),
        operation,
        crate::application::dictation_tail_diagnostics::DictationStopReason::Manual,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
