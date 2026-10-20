//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
mod app_runtime;
pub mod application;
pub mod audio;
mod autostart;
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
    logging::install_crash_hook();
    let Ok(context) = single_instance::context(tauri::generate_context!()) else {
        eprintln!("Не удалось подготовить изолированный профиль Fono.");
        return;
    };
    let Ok(startup_gate) = single_instance::startup_gate(&context.config().identifier) else {
        eprintln!("Не удалось дождаться запуска Fono. Повторите запуск приложения.");
        return;
    };
    let app = tauri::Builder::default()
        .plugin(single_instance::plugin())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(app_runtime::setup)
        .invoke_handler(ipc::handler())
        .build(context);
    drop(startup_gate);
    let Ok(app) = app else {
        tracing::error!("Не удалось запустить Fono. Проверьте доступ к профилю приложения.");
        return;
    };
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            app_runtime::shutdown(app);
        }
    });
}
