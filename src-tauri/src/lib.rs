//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
mod app_runtime;
pub mod application;
pub mod audio;
mod autostart;
mod cli;
mod crash_log;
pub mod error;
pub mod events;
pub mod history;
pub mod injection;
pub mod ipc;
pub mod llm;
mod logging;
pub mod overlay;
mod overlay_geometry;
pub mod pipeline;
pub mod secrets;
pub mod service_history;
mod single_instance;
pub mod speech_metrics;
pub mod state;
pub mod stt;
mod tray;
pub mod types;
pub mod vad;
pub mod verbose;

#[cfg(all(test, windows))]
#[path = "stt/lifecycle_soak_win32.rs"]
mod test_process;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let command = match cli::launch_command() {
        Ok(command) => command,
        Err(message) => {
            cli::argument_error(message);
            std::process::exit(2);
        }
    };
    logging::install_crash_hook();
    let Ok(mut context) = single_instance::context(tauri::generate_context!()) else {
        eprintln!("Не удалось подготовить изолированный профиль Fono.");
        return;
    };
    cli::configure_launch(&mut context, command);
    let Ok(startup_gate) = single_instance::startup_gate(&context.config().identifier) else {
        eprintln!("Не удалось дождаться запуска Fono. Повторите запуск приложения.");
        return;
    };
    let app = tauri::Builder::default()
        .plugin(single_instance::plugin())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            app_runtime::setup(app)?;
            if let Err(error) = cli::initialize(app.handle(), command) {
                // A bad/busy CLI action must not tear down a usable primary runtime.
                crate::events::emit_error(
                    app.handle(),
                    crate::events::ErrorCodeV1::Internal,
                    error.to_string(),
                    None,
                );
            }
            Ok(())
        })
        .invoke_handler(ipc::handler())
        .build(context);
    drop(startup_gate);
    let Ok(app) = app else {
        tracing::error!("Не удалось запустить Fono. Проверьте доступ к профилю приложения.");
        return;
    };
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Ready) {
            cli::ready(app);
        }
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            app_runtime::shutdown(app);
        }
    });
}
