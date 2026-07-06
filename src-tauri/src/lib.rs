//! WhisperClone — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod audio;
pub mod commands;
pub mod error;
pub mod injection;
pub mod llm;
pub mod pipeline;
pub mod state;
pub mod stt;
pub mod types;

use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use crate::state::AppState;

/// Инициализация логирования: консоль + файл в `%APPDATA%\WhisperClone\logs\`.
fn init_tracing() {
    use tracing_appender::rolling;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,whisperclone=debug"));

    // Файловый appender: один файл в день, в папке logs рядом с настройками.
    let log_dir = state::app_data_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = rolling::daily(&log_dir, "whisperclone.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    // guard нужно держать вечно — отдадим в leak, чтобы не дропнуть.
    std::mem::forget(guard);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // без цветовых ESC-кодов в файле
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let console_layer = fmt::layer()
        .with_target(false)
        .with_file(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .try_init();

    tracing::info!(
        "log file: {}",
        log_dir.join("whisperclone.log").display()
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    tracing::info!("WhisperClone starting up…");

    let app_state = state::AppState::new();
    let pipeline = pipeline::Pipeline::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state)
        .manage(pipeline)
        .setup(|app| {
            // Трей-иконка с меню
            setup_tray(app)?;
            setup_global_shortcut(app)?;
            // Запуск фонового конвейера
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = pipeline::start_background(handle).await {
                    tracing::error!("pipeline crashed: {e:?}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // state
            commands::get_pipeline_state,
            commands::start_dictation,
            commands::stop_dictation,
            commands::transcribe_test,
            // audio
            commands::list_audio_devices,
            // whisper
            commands::list_whisper_models,
            commands::download_whisper_model,
            commands::set_whisper_model,
            // llm
            commands::test_llm_connection,
            // settings
            commands::get_settings,
            commands::save_settings,
            // диагностика
            commands::get_recent_logs,
            commands::test_microphone,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperClone");
}

/// Создаёт системный трей с базовым меню.
fn setup_global_shortcut(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();
    let hotkey = app_handle.state::<AppState>().settings().hotkey;

    app_handle
        .global_shortcut()
        .on_shortcut(hotkey.as_str(), move |app, _, event| {
            match event.state {
                ShortcutState::Pressed => {
                    if let Err(e) = commands::start_dictation(app.clone()) {
                        let _ = app.emit("error", e.to_string());
                        tracing::warn!("start_dictation via global shortcut failed: {e}");
                    }
                }
                ShortcutState::Released => {
                    if app.state::<pipeline::Pipeline>().is_recording() {
                        let app_for_stop = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = commands::stop_dictation(app_for_stop).await {
                                tracing::warn!("stop_dictation via global shortcut failed: {e}");
                            }
                        });
                    }
                }
            }
        })?;

    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItemBuilder::with_id("show", "Открыть настройки").build(app)?;
    let pause = MenuItemBuilder::with_id("pause", "Пауза").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Выход").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&pause)
        .separator()
        .item(&quit)
        .build()?;

    let pause_item = pause.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("WhisperClone")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "pause" => {
                let state = app.state::<AppState>();
                let pipeline = app.state::<pipeline::Pipeline>();
                let is_paused = state.toggle_dictation_paused();
                if is_paused {
                    if pipeline.is_recording() {
                        match pipeline.stop_recording() {
                            Ok(samples) => {
                                let _ = samples;
                                pipeline::set_state(&app, &state.inner(), crate::types::PipelineState::Idle);
                            }
                            Err(e) => {
                                let _ = app.emit("error", e.to_string());
                                tracing::warn!("pause stop_recording failed: {e}");
                            }
                        }
                    }
                    let _ = pause_item.set_text("Resume");
                    tracing::info!("dictation paused");
                } else {
                    let _ = pause_item.set_text("Pause");
                    tracing::info!("dictation resumed");
                }
            }
            "quit" => {
                tracing::info!("Quit requested from tray");
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
