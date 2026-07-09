//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
pub mod audio;
pub mod commands;
pub mod error;
pub mod injection;
pub mod llm;
pub mod pipeline;
pub mod state;
pub mod stt;
pub mod types;
pub mod vad;
pub mod verbose;

use crate::state::AppState;
use crate::types::{PipelineState, Settings, WakeWordBackend};
use fono_wake::{WakeWordConfig, WakeWordEvent, WakeWordHandle};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Инициализация логирования: консоль + файл в `%APPDATA%\Fono\logs\`.
fn init_tracing() {
    use tracing_appender::rolling;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,fono=debug"));

    // Файловый appender: один файл в день, в папке logs рядом с настройками.
    let log_dir = state::app_data_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = rolling::daily(&log_dir, "fono.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    // guard нужно держать вечно — отдадим в leak, чтобы не дропнуть.
    std::mem::forget(guard);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // без цветовых ESC-кодов в файле
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let console_layer = fmt::layer().with_target(false).with_file(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .try_init();

    tracing::info!("log file: {}", log_dir.join("fono.log").display());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    tracing::info!("Fono starting up…");

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
        .manage(WakeWordHandle::new(WakeWordConfig::default()))
        .setup(|app| {
            // Трей-иконка с меню
            setup_tray(app)?;
            // Push-to-talk: Ctrl+Space (Pressed) → запись, (Released) → стоп + STT + вставка.
            setup_global_shortcut(app)?;

            // Закрытие окна настроек сворачивает его в трей, а не уничтожает.
            // Это позволяет снова открыть окно из трея.
            if let Some(settings_window) = app.get_webview_window("settings") {
                let settings_window_clone = settings_window.clone();
                settings_window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = settings_window_clone.hide();
                        tracing::info!("settings window hidden to tray");
                    }
                });
            }

            // Восстанавливаем позицию overlay-окна из настроек.
            let settings = app.state::<AppState>().settings();
            crate::verbose::set_verbose(settings.verbose_logging);
            if let (Some(x), Some(y)) = (settings.overlay_x, settings.overlay_y) {
                if let Some(overlay) = app.get_webview_window("overlay") {
                    let _ = overlay.set_position(tauri::PhysicalPosition::new(x, y));
                }
            }

            // Wake word: запускаем, если включён в настройках.
            // Загружает base-модель для транскрипции чанков (баланс скорости/точности).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_wake_word_if_enabled(&handle).await {
                    tracing::error!("wake word start failed: {e:?}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // state
            commands::get_pipeline_state,
            commands::start_dictation,
            commands::stop_dictation,
            commands::confirm_dictation,
            commands::cancel_dictation,
            commands::transcribe_test,
            // audio
            commands::list_audio_devices,
            // whisper
            commands::list_whisper_models,
            commands::download_whisper_model,
            commands::set_whisper_model,
            // kws wake word model
            commands::is_kws_model_downloaded,
            commands::download_kws_model,
            // llm
            commands::test_llm_connection,
            commands::list_llm_models,
            // settings
            commands::get_settings,
            commands::save_settings,
            // overlay
            commands::save_overlay_position,
            // диагностика
            commands::get_recent_logs,
            commands::clear_logs,
            commands::test_microphone,
            // wake word
            commands::get_wake_word_status,
            commands::enable_wake_word,
            commands::disable_wake_word,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fono");
}

/// Регистрирует глобальные горячие клавиши:
/// - push-to-talk (dictation)
/// - voice commands
fn setup_global_shortcut(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();
    let settings = app_handle.state::<AppState>().settings();
    if let Err(e) = register_all_shortcuts(&app_handle, &settings) {
        tracing::error!("Не удалось зарегистрировать горячие клавиши: {e}");
        tracing::error!("Возможно, одна из клавиш уже занята другим приложением.");
    }
    Ok(())
}

/// Регистрирует (или перерегистрирует) все глобальные шорткаты.
/// Сначала отменяет все текущие, затем регистрирует push-to-talk и command hotkey.
pub fn register_all_shortcuts(
    app: &tauri::AppHandle,
    settings: &crate::types::Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    // Push-to-talk: зажатие → запись, отпускание → стоп + STT + вставка.
    gs.on_shortcut(settings.hotkey.as_str(), |app, _, event| {
        match event.state {
            ShortcutState::Pressed => {
                let _ = app.emit("pipeline-mode", "dictation");
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
    tracing::info!("push-to-talk hotkey '{}' registered", settings.hotkey);

    // Voice commands: зажатие → запись, отпускание → стоп + STT + выполнение команды.
    let command_hotkey = settings.command_hotkey.clone();
    gs.on_shortcut(command_hotkey.as_str(), |app, _, event| match event.state {
        ShortcutState::Pressed => {
            let _ = app.emit("pipeline-mode", "command");
            if let Err(e) = commands::start_dictation(app.clone()) {
                let _ = app.emit("error", e.to_string());
                tracing::warn!("start voice command recording failed: {e}");
            }
        }
        ShortcutState::Released => {
            if app.state::<pipeline::Pipeline>().is_recording() {
                let app_for_command = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = run_voice_command(&app_for_command).await {
                        tracing::warn!("voice command failed: {e}");
                        let _ = app_for_command.emit("error", e.to_string());
                    }
                });
            }
        }
    })?;
    tracing::info!("command hotkey '{}' registered", command_hotkey);

    Ok(())
}

/// Полный цикл голосовой команды: запись → STT → выполнение.
async fn run_voice_command(
    app: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tauri::Manager;
    let state = app.state::<state::AppState>();
    let pipeline = app.state::<pipeline::Pipeline>();
    let settings = state.settings();

    let samples = pipeline.stop_recording()?;
    if samples.is_empty() {
        pipeline::set_state(app, &state.inner(), PipelineState::Idle);
        return Ok(());
    }

    let samples = crate::vad::trim_silence(&samples);
    if samples.is_empty() {
        pipeline::set_state(app, &state.inner(), PipelineState::Idle);
        return Ok(());
    }

    // Загружаем основную whisper-модель.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        pipeline
            .stt()
            .ensure_loaded(std::path::Path::new(path), settings.use_gpu)?;
    } else {
        pipeline::set_state(app, &state.inner(), PipelineState::Idle);
        return Err("Whisper-модель не выбрана".into());
    }

    pipeline::set_state(app, &state.inner(), PipelineState::Transcribing);
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let transcript =
        tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
            .await
            .map_err(|e| format!("transcribe join: {e}"))??;

    tracing::info!("voice command transcript: {:?}", transcript.text);

    pipeline::set_state(app, &state.inner(), PipelineState::Processing);
    match crate::app_commands::execute(&transcript.text, &settings.launch_apps, settings.volume_step) {
        Ok(result) => {
            tracing::info!("voice command result: {result}");
            let _ = app.emit("command-result", result);
        }
        Err(e) => {
            tracing::warn!("voice command execute failed: {e}");
            let _ = app.emit("error", e.to_string());
        }
    }

    pipeline::set_state(app, &state.inner(), PipelineState::Idle);
    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let initial_wake = app.state::<AppState>().settings().wake_word_enabled;
    let wake_label = if initial_wake {
        "Выключить wake word"
    } else {
        "Включить wake word"
    };

    let show = MenuItemBuilder::with_id("show", "Открыть настройки").build(app)?;
    let pause = MenuItemBuilder::with_id("pause", "Пауза").build(app)?;
    let wake_word = MenuItemBuilder::with_id("wake_word", wake_label).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Выход").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&pause)
        .separator()
        .item(&wake_word)
        .separator()
        .item(&quit)
        .build()?;

    let pause_item = pause.clone();
    let wake_item = wake_word.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Fono")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.unminimize();
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
                                pipeline::set_state(
                                    &app,
                                    &state.inner(),
                                    crate::types::PipelineState::Idle,
                                );
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
            "wake_word" => {
                let app_clone = app.clone();
                let wake_item_clone = wake_item.clone();
                let is_enabled = app.state::<AppState>().settings().wake_word_enabled;
                tauri::async_runtime::spawn(async move {
                    let result = if is_enabled {
                        commands::disable_wake_word(app_clone.clone()).await
                    } else {
                        commands::enable_wake_word(app_clone.clone()).await
                    };
                    match result {
                        Ok(_) => {
                            let new_label = if is_enabled {
                                "Включить wake word"
                            } else {
                                "Выключить wake word"
                            };
                            let _ = wake_item_clone.set_text(new_label);
                        }
                        Err(e) => {
                            tracing::warn!("tray wake word toggle failed: {e}");
                            let _ = app_clone.emit("error", e.to_string());
                        }
                    }
                });
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

/// Builds a backend-specific wake word config from application settings.
fn settings_to_wake_config(settings: &Settings) -> crate::error::AppResult<WakeWordConfig> {
    let mut config = WakeWordConfig::default();
    config.enabled = settings.wake_word_enabled;
    config.backend = settings.wake_backend;
    config.phrase = settings.wake_word.clone();
    config.audio_device_id = settings.audio_device_id.clone();
    config.sample_rate = 16_000;
    config.threshold = settings.wake_word_threshold;
    config.sensitivity = settings.wake_word_sensitivity;
    config.cooldown_ms = 2_000;
    config.model_dir = match settings.wake_backend {
        WakeWordBackend::SherpaOnnx => state::app_data_dir()?
            .join("kws-models")
            .join("sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01"),
        WakeWordBackend::WhisperExperimental => {
            state::models_dir()?.join(settings.wake_word_model.filename())
        }
        _ => std::path::PathBuf::new(),
    };
    Ok(config)
}

/// Запускает wake word детектор, если он включён в настройках.
///
/// Использует выбранный backend (`sherpa-onnx` по умолчанию).
/// При обнаружении wake-фразы стартует запись диктовки,
/// по тишине (VAD) — STT (основной моделью) + вставка текста.
async fn start_wake_word_if_enabled(
    handle: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let settings = state.settings();

    if !settings.wake_word_enabled {
        tracing::info!("wake word: disabled in settings, skipping");
        return Ok(());
    }

    let wake_handle = handle.state::<WakeWordHandle>();
    let handle_clone = handle.clone();
    wake_handle.set_callback(move |event| match event {
        WakeWordEvent::Detected { phrase } => {
            tracing::info!("wake word detected: {phrase}");
            let _ = handle_clone.emit("wake-word-detected", &phrase);
            let h = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = run_dictation_after_wake(&h).await {
                    tracing::error!("dictation after wake failed: {e:?}");
                    let _ = h.emit("error", e.to_string());
                }
            });
        }
        WakeWordEvent::Error { message } => {
            tracing::error!("wake word error: {message}");
            let _ = handle_clone.emit("error", &message);
        }
        WakeWordEvent::Listening => {
            let _ = handle_clone.emit("wake-word-status", "listening");
        }
        WakeWordEvent::Paused => {
            let _ = handle_clone.emit("wake-word-status", "paused");
        }
        WakeWordEvent::ModelLoading => {
            let _ = handle_clone.emit("wake-word-status", "loading");
        }
        WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            let _ = handle_clone.emit("wake-word-status", "missing_model");
            let _ = handle_clone.emit(
                "error",
                format!("Wake word: модель не найдена. Скачайте её в настройках: {path}"),
            );
        }
    });

    let config = settings_to_wake_config(&settings)?;
    wake_handle.update_config(config)?;

    Ok(())
}

/// Перезапускает wake word детектор (без изменения флага wake_word_enabled).
/// Используется когда пользователь меняет настройки wake word "на лету".
pub async fn restart_wake_word(handle: &tauri::AppHandle) -> Result<(), String> {
    let wake_handle = handle.state::<WakeWordHandle>();
    let settings = handle.state::<state::AppState>().settings();
    let config = settings_to_wake_config(&settings).map_err(|e| e.to_string())?;
    wake_handle.update_config(config).map_err(|e| e.to_string())
}

/// Запускает диктовку после срабатывания wake word.
///
/// Алгоритм:
///   1. Паузим wake word (чтобы не ловить повторные срабатывания).
///   2. Стартуем запись.
///   3. Слушаем VAD: ждём пока пользователь говорит, потом тишина 1.5 сек → стоп.
///   4. STT основной моделью + вставка текста.
///   5. Резюммим wake word.
pub async fn run_dictation_after_wake(
    handle: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let pipeline = handle.state::<pipeline::Pipeline>();
    let wake_handle = handle.state::<WakeWordHandle>();
    let settings = state.settings();

    // Паузим wake word.
    wake_handle.pause();

    // Стартуем запись.
    pipeline::set_state(handle, &state.inner(), PipelineState::Listening);
    let _ = handle.emit("pipeline-mode", "dictation");
    pipeline.start_recording(settings.audio_device_id.as_deref())?;
    tracing::info!("wake dictation: recording started, waiting for VAD silence");

    // Ждём окончания речи: ловим начало речи, затем остановку по тишине.
    // Уровень звука читаем из writer-буфера записи (см. Pipeline::current_level).
    let max_wait = std::time::Duration::from_secs(30); // максимум 30 сек диктовки
    let silence_level = 0.012; // ~-38 dBFS, как DEFAULT_THRESHOLD в vad/mod.rs
    let silence_timeout = std::time::Duration::from_millis(1000); // тишина после речи → стоп
    let started = std::time::Instant::now();
    let mut was_speaking = false;
    let mut silence_start: Option<std::time::Instant> = None;

    loop {
        if started.elapsed() >= max_wait || pipeline.is_cancelled() || pipeline.is_confirmed() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let level = pipeline.current_level();
        if level > silence_level {
            // Идёт речь — сбрасываем счётчик тишины.
            was_speaking = true;
            silence_start = None;
        } else if was_speaking && silence_start.is_none() {
            // Речь была, началась тишина — запускаем таймер.
            silence_start = Some(std::time::Instant::now());
            tracing::debug!("wake dictation: silence started (level={:.4})", level);
        }

        // Тишина длится дольше порога после речи → останавливаем запись.
        if let Some(s) = silence_start {
            if s.elapsed() >= silence_timeout {
                tracing::info!(
                    "wake dictation: silence {:.1}s reached, stopping",
                    s.elapsed().as_secs_f32()
                );
                break;
            }
        }
    }

    tracing::info!(
        "wake dictation: stopping recording after {:.1}s (was_speaking={})",
        started.elapsed().as_secs_f32(),
        was_speaking
    );

    // Стоп + STT + вставка.
    let samples = pipeline.stop_recording()?;

    // Сбрасываем флаг подтверждения, чтобы не влиял на следующие вызовы.
    pipeline.reset_confirm();

    // Если пользователь нажал Stop в оверлее — отбрасываем запись.
    if pipeline.is_cancelled() {
        tracing::info!("wake dictation: cancelled by user");
        pipeline::set_state(handle, &state.inner(), PipelineState::Idle);
        wake_handle.resume();
        return Ok(());
    }

    if !samples.is_empty() {
        let samples = crate::vad::trim_silence(&samples);
        if !samples.is_empty() {
            pipeline::set_state(handle, &state.inner(), PipelineState::Transcribing);
            let stt = pipeline.stt().clone();
            let language = settings.language.clone();
            let transcript =
                tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
                    .await
                    .map_err(|e| format!("transcribe join: {e}"))??;

            tracing::info!("wake dictation transcript: {:?}", transcript.text);

            // Опциональная AI-обработка.
            let final_text = match settings.ai_mode {
                crate::types::AiMode::Off => transcript.text.clone(),
                mode => {
                    pipeline::set_state(handle, &state.inner(), PipelineState::Processing);
                    let client = crate::llm::LlmClient::from_settings(&settings);
                    match client
                        .process(&transcript.text, mode, settings.clean_prompt.as_deref())
                        .await
                    {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!("LLM failed ({e}) — raw transcript");
                            transcript.text.clone()
                        }
                    }
                }
            };

            if !final_text.is_empty() {
                pipeline::set_state(handle, &state.inner(), PipelineState::Injecting);
                if let Err(e) = crate::injection::inject_text(&final_text, settings.injection_mode)
                {
                    tracing::warn!("injection failed: {e}");
                    let _ = handle.emit("error", format!("Вставка: {e}"));
                }
            }
        }
    }

    pipeline::set_state(handle, &state.inner(), PipelineState::Idle);

    // Резюммим wake word.
    wake_handle.resume();

    Ok(())
}
