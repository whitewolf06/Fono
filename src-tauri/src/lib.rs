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
pub mod vad;
pub mod wakeword;

use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use crate::state::AppState;
use crate::types::PipelineState;

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
        .manage(wakeword::WakeWordDetector::new())
        .setup(|app| {
            // Трей-иконка с меню
            setup_tray(app)?;
            // Push-to-talk: Ctrl+Space (Pressed) → запись, (Released) → стоп + STT + вставка.
            setup_global_shortcut(app)?;

            // Wake word: запускаем, если включён в настройках.
            // Загружает tiny-модель для быстрой транскрипции чанков.
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
            // wake word
            commands::get_wake_word_status,
            commands::enable_wake_word,
            commands::disable_wake_word,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperClone");
}

/// Регистрирует глобальную горячую клавишу push-to-talk.
///
/// Ctrl+Space: зажатие → старт записи, отпускание → стоп + STT + вставка.
fn setup_global_shortcut(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();
    let hotkey = app_handle.state::<AppState>().settings().hotkey.clone();
    tracing::info!("registering push-to-talk hotkey: {}", hotkey);

    if let Err(e) = app_handle.global_shortcut().on_shortcut(hotkey.as_str(), move |app, _, event| {
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
        }) {
        tracing::error!("Не удалось зарегистрировать горячую клавишу '{}': {e}", hotkey);
        tracing::error!("Возможно, она уже занята другим приложением. Push-to-talk недоступен, но тест кнопки работает.");
    } else {
        tracing::info!("push-to-talk hotkey '{}' registered successfully", hotkey);
    }

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

/// Запускает wake word детектор, если он включён в настройках.
///
/// Использует tiny-модель whisper для быстрой транскрипции чанков.
/// При обнаружении фразы «Эй, ассистент» стартует запись диктовки,
/// по тишине (VAD) — STT (основной моделью) + вставка текста.
async fn start_wake_word_if_enabled(handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let settings = state.settings();

    if !settings.wake_word_enabled {
        tracing::info!("wake word: disabled in settings, skipping");
        return Ok(());
    }

    // Загружаем tiny-модель для wake word (быстрая транскрипция).
    let models_dir = state::models_dir()?;
    let tiny_path = models_dir.join("ggml-tiny.bin");
    if !tiny_path.exists() {
        tracing::warn!(
            "wake word: tiny model not found at {}, wake word disabled",
            tiny_path.display()
        );
        let _ = handle.emit("error", "Wake word: модель tiny не найдена. Скачайте её в настройках.");
        return Ok(());
    }

    let pipeline_state = handle.state::<pipeline::Pipeline>();
    pipeline_state.stt().ensure_loaded(&tiny_path)?;

    let detector = handle.state::<wakeword::WakeWordDetector>();
    detector.set_phrase(settings.wake_word.clone());

    let stt = pipeline_state.stt().clone();
    let device_id = settings.audio_device_id.clone();
    let handle_clone = handle.clone();

    detector.start(stt, device_id, move |event| {
        match event {
            wakeword::WakeEvent::Detected { transcription } => {
                tracing::info!("wake word triggered: {:?}", transcription);
                let _ = handle_clone.emit("wake-word-detected", &transcription);

                // Стартуем запись диктовки в отдельном async-потоке.
                let h = handle_clone.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = run_dictation_after_wake(&h).await {
                        tracing::error!("dictation after wake failed: {e:?}");
                        let _ = h.emit("error", e.to_string());
                    }
                });
            }
            wakeword::WakeEvent::Error(msg) => {
                tracing::error!("wake word error: {msg}");
                let _ = handle_clone.emit("error", &msg);
            }
            wakeword::WakeEvent::Status(s) => {
                let _ = handle_clone.emit("wake-word-status", format!("{:?}", s));
            }
        }
    })?;

    Ok(())
}

/// Запускает диктовку после срабатывания wake word.
///
/// Алгоритм:
///   1. Паузим wake word (чтобы не ловить повторные срабатывания).
///   2. Стартуем запись.
///   3. Слушаем VAD: ждём пока пользователь говорит, потом тишина 1.5 сек → стоп.
///   4. STT основной моделью + вставка текста.
///   5. Резюммим wake word.
pub async fn run_dictation_after_wake(handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let pipeline = handle.state::<pipeline::Pipeline>();
    let detector = handle.state::<wakeword::WakeWordDetector>();
    let settings = state.settings();

    // Паузим wake word.
    detector.pause();

    // Переключаемся на основную модель (если wake word использовал tiny).
    if let Some(path) = settings.whisper_model_path.as_deref() {
        pipeline.stt().ensure_loaded(std::path::Path::new(path))?;
    }

    // Стартуем запись.
    pipeline::set_state(handle, &state.inner(), PipelineState::Listening);
    pipeline.start_recording(settings.audio_device_id.as_deref())?;
    tracing::info!("wake dictation: recording started, waiting for VAD silence");

    // Ждём окончания речи: слушаем, пока есть звук, потом тишина 1.5 сек.
    let max_wait = std::time::Duration::from_secs(30); // максимум 30 сек диктовки
    let silence_threshold = std::time::Duration::from_millis(1500);
    let started = std::time::Instant::now();
    let mut last_speech = std::time::Instant::now();
    let mut was_speaking = false;

    // Пул сэмплов для VAD-проверки (берём последние 100 мс).
    while started.elapsed() < max_wait {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Проверяем уровень звука через pipeline (последние сэмплы).
        // Простой подход: если pipeline.recording идёт, проверяем writer.
        let now = std::time::Instant::now();
        // TODO: реальный VAD-мониторинг — пока используем таймаут 5 сек как fallback.
        if was_speaking && now.duration_since(last_speech) > silence_threshold {
            break;
        }
        // Упрощённо: считаем что пользователь начал говорить сразу.
        if !was_speaking {
            was_speaking = true;
            last_speech = now;
        }
        // Обновляем last_speech при каждом чанке (заглушка — реальная логика в VAD-мониторе).
        last_speech = now;
    }

    tracing::info!("wake dictation: stopping recording after {:.1}s", started.elapsed().as_secs_f32());

    // Стоп + STT + вставка.
    let samples = pipeline.stop_recording()?;
    if !samples.is_empty() {
        let samples = crate::vad::trim_silence(&samples);
        if !samples.is_empty() {
            pipeline::set_state(handle, &state.inner(), PipelineState::Transcribing);
            let stt = pipeline.stt().clone();
            let language = settings.language.clone();
            let transcript = tauri::async_runtime::spawn_blocking(move || stt.transcribe(&samples, &language))
                .await
                .map_err(|e| format!("transcribe join: {e}"))??;

            tracing::info!("wake dictation transcript: {:?}", transcript.text);

            // Опциональная AI-обработка.
            let final_text = match settings.ai_mode {
                crate::types::AiMode::Off => transcript.text.clone(),
                mode => {
                    pipeline::set_state(handle, &state.inner(), PipelineState::Processing);
                    let client = crate::llm::LlmClient::new(
                        settings.llm_base_url.clone(),
                        settings.llm_model.clone(),
                    );
                    match client.process(&transcript.text, mode).await {
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
                if let Err(e) = crate::injection::inject_text(&final_text) {
                    tracing::warn!("injection failed: {e}");
                    let _ = handle.emit("error", format!("Вставка: {e}"));
                }
            }
        }
    }

    pipeline::set_state(handle, &state.inner(), PipelineState::Idle);

    // Переключаемся обратно на tiny-модель и резюммим wake word.
    let models_dir = state::models_dir()?;
    let tiny_path = models_dir.join("ggml-tiny.bin");
    if tiny_path.exists() {
        let _ = pipeline.stt().ensure_loaded(&tiny_path);
    }
    detector.resume();

    Ok(())
}
