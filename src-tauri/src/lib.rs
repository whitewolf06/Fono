//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
pub mod application;
pub mod audio;
pub mod error;
pub mod events;
pub mod history;
pub mod injection;
pub mod ipc;
pub mod llm;
pub mod operation;
pub mod pipeline;
pub mod secrets;
pub mod service_history;
pub mod speech_metrics;
pub mod state;
pub mod stt;
pub mod types;
pub mod vad;
pub mod verbose;

use crate::operation::{OperationSource, TerminalReason};
use crate::state::AppState;
use crate::types::{PipelineState, Settings, WakeWordBackend};
use fono_wake::{AudioHub, WakeWordConfig, WakeWordEvent, WakeWordHandle};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Состояние таймера окончания записи после wake word.
///
/// Отправляется только для диктовки, запущенной wake word: обычная запись по
/// горячей клавише не должна показывать этот обратный отсчёт.
fn emit_wake_dictation_countdown(
    handle: &tauri::AppHandle,
    remaining: std::time::Duration,
    timeout: std::time::Duration,
    speaking: bool,
) {
    events::emit_wake_countdown(
        handle,
        events::WakeCountdownV1 {
            remaining_ms: remaining.as_millis() as u64,
            timeout_ms: timeout.as_millis() as u64,
            speaking,
        },
    );
}

/// Restores wake-word listening when a wake-triggered operation leaves scope.
/// The guard deliberately owns no operation state: it only pairs the engine
/// pause with its mandatory resume across every early return and await point.
trait WakeLifecycle {
    fn pause(&self);
    fn resume(&self);
}

impl WakeLifecycle for WakeWordHandle {
    fn pause(&self) {
        WakeWordHandle::pause(self);
    }

    fn resume(&self) {
        WakeWordHandle::resume(self);
    }
}

struct WakePauseGuard<'a, T: WakeLifecycle> {
    wake_handle: &'a T,
}

impl<'a, T: WakeLifecycle> WakePauseGuard<'a, T> {
    fn pause(wake_handle: &'a T) -> Self {
        wake_handle.pause();
        Self { wake_handle }
    }
}

impl<T: WakeLifecycle> Drop for WakePauseGuard<'_, T> {
    fn drop(&mut self) {
        self.wake_handle.resume();
    }
}

/// Возвращает действие, явно продиктованное после wake phrase.
///
/// Намеренно не пытаемся угадывать команды из обычного текста: системные
/// действия должны требовать явного префикса «команда». Результат в нижнем
/// регистре безопасен для текущего сопоставления окон и приложений.
fn extract_wake_command(transcript: &str) -> Option<String> {
    let normalized = transcript.trim().to_lowercase();
    for prefix in ["команда", "выполни команду", "command"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let command = rest
                .trim_start_matches(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | ',' | '.' | '-')
                })
                .trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }
    None
}

/// Removes only a leading wake phrase from the main dictation transcript.
/// Audio pre-roll intentionally preserves the first user words; this textual
/// guard prevents the detector phrase from leaking into the injected result.
fn strip_leading_wake_phrase(transcript: &str, configured_phrase: &str) -> String {
    let trimmed = transcript.trim_start();
    let words: Vec<&str> = trimmed.split_whitespace().take(2).collect();
    if words.len() < 2 {
        return trimmed.to_string();
    }
    let normalize_word = |word: &str| {
        word.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let first = normalize_word(words[0]);
    let second = normalize_word(words[1]);
    let configured = configured_phrase.to_lowercase();
    let matched = match configured.as_str() {
        "okay fun" => {
            matches!(first.as_str(), "okay" | "ok" | "okey" | "окей")
                && matches!(second.as_str(), "fun" | "fan" | "фан" | "фэн")
        }
        "hey fono" => {
            matches!(first.as_str(), "hey" | "hi" | "she" | "хей")
                && matches!(
                    second.as_str(),
                    "fono" | "phono" | "phone" | "фоно" | "фона"
                )
        }
        _ => {
            let expected: Vec<String> = configured.split_whitespace().map(normalize_word).collect();
            expected.len() == 2 && expected[0] == first && expected[1] == second
        }
    };
    if !matched {
        return trimmed.to_string();
    }

    let mut rest = trimmed;
    for word in words {
        rest = rest.strip_prefix(word).unwrap_or(rest);
        rest = rest.trim_start();
    }
    rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | ',' | '.' | '-'))
        .trim_start()
        .to_string()
}

/// Инициализация логирования: консоль + файл в `%APPDATA%\Fono\logs\`.
fn init_tracing() {
    use tracing_appender::rolling;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,fono=debug"));

    // Файловый appender: один файл в день, храним не более двух недель.
    let log_dir = state::app_data_dir()
        .map(|d| d.join("logs"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("fono.log")
        .max_log_files(14)
        .build(&log_dir)
        .unwrap_or_else(|error| panic!("cannot initialize bounded file logging: {error}"));
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
    let audio_hub = AudioHub::new();
    let pipeline = pipeline::Pipeline::new_with_audio_hub(audio_hub.clone());
    let wake_word =
        WakeWordHandle::new_with_audio_hub(WakeWordConfig::default(), audio_hub.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state)
        .manage(pipeline)
        .manage(wake_word)
        .manage(audio_hub)
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
            if let Err(error) = crate::history::apply_analytics_privacy_policy(
                settings.analytics_enabled && settings.history_enabled,
                settings.analytics_retention_days,
            ) {
                tracing::warn!(%error, "could not apply analytics privacy policy at startup");
            }
            let readiness_events = app.handle().clone();
            app.state::<pipeline::Pipeline>()
                .stt()
                .set_readiness_observer(std::sync::Arc::new(move |readiness| {
                    events::emit_stt_readiness(&readiness_events, readiness);
                }));
            crate::application::models::preload_configured_stt(app.handle().clone());
            let local_service =
                crate::application::local_transcription_service::LocalTranscriptionService::start(
                    app.handle().clone(),
                )?;
            app.manage(local_service);
            app.manage(
                crate::application::speech_analysis_queue::SpeechAnalysisQueue::start(
                    app.handle().clone(),
                ),
            );
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
            ipc::system::get_build_info,
            ipc::dictation::get_pipeline_state,
            ipc::dictation::start_dictation,
            ipc::dictation::stop_dictation,
            ipc::dictation::confirm_dictation,
            ipc::dictation::cancel_dictation,
            ipc::dictation::transcribe_test,
            ipc::system::get_dictation_history,
            ipc::system::clear_dictation_history,
            ipc::system::delete_dictation_history_entry,
            ipc::system::get_speech_session_analysis,
            ipc::system::get_speech_period_report,
            ipc::service::get_local_transcription_service_snapshot,
            ipc::service::cancel_local_transcription_job,
            ipc::service::clear_local_transcription_history,
            ipc::service::copy_local_transcription_api_token,
            ipc::text::copy_dictation_text,
            ipc::text::reinsert_dictation,
            ipc::voice::get_pending_voice_command,
            ipc::voice::get_pending_command_proposal,
            ipc::voice::cancel_voice_command,
            ipc::voice::confirm_voice_command,
            // audio
            ipc::audio::list_audio_devices,
            // whisper
            ipc::models::list_whisper_models,
            ipc::models::download_whisper_model,
            ipc::models::set_whisper_model,
            // kws wake word model
            ipc::models::is_kws_model_downloaded,
            ipc::models::download_kws_model,
            ipc::models::cancel_model_download,
            // llm
            ipc::llm::test_llm_connection,
            ipc::llm::list_llm_models,
            ipc::llm::test_llm_profile,
            ipc::llm::list_llm_profile_models,
            // settings
            ipc::settings::get_settings,
            ipc::settings::get_acceleration_capabilities,
            ipc::settings::get_stt_readiness,
            ipc::settings::get_stt_health,
            ipc::settings::save_settings,
            // overlay
            ipc::settings::save_overlay_position,
            // диагностика
            ipc::diagnostics::get_recent_logs,
            ipc::diagnostics::clear_logs,
            ipc::diagnostics::test_microphone,
            // wake word
            ipc::wake::get_wake_word_status,
            ipc::wake::get_wake_word_capabilities,
            ipc::wake::get_wake_word_diagnostics,
            ipc::wake::test_wake_word_model,
            ipc::wake::record_wake_word_sample,
            ipc::wake::recognize_wake_word_sample,
            ipc::wake::enable_wake_word,
            ipc::wake::disable_wake_word,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Fono")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. }) {
                shutdown_app(app);
            }
        });
}

fn shutdown_app(app: &tauri::AppHandle) {
    tracing::info!("Fono shutdown requested");
    app.state::<crate::application::local_transcription_service::LocalTranscriptionService>()
        .shutdown();
    let pipeline = app.state::<pipeline::Pipeline>();
    if let Some(event) = pipeline.shutdown() {
        events::emit_operation(app, event);
    }
    injection::shutdown();
    app.state::<WakeWordHandle>().stop();
    app.state::<AudioHub>().shutdown();
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    pipeline::set_state(app, app.state::<AppState>().inner(), PipelineState::Idle);
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
                events::emit_pipeline_mode(app, events::PipelineModeV1::Dictation);
                if let Err(e) =
                    ipc::dictation::start_dictation_from(app.clone(), OperationSource::Hotkey)
                {
                    events::emit_error(app, events::ErrorCodeV1::Audio, e.to_string(), None);
                    tracing::warn!("start_dictation via global shortcut failed: {e}");
                }
            }
            ShortcutState::Released => {
                if app.state::<pipeline::Pipeline>().is_recording() {
                    let app_for_stop = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = application::dictation::stop(app_for_stop).await {
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
            events::emit_pipeline_mode(app, events::PipelineModeV1::Command);
            if let Err(e) =
                ipc::dictation::start_dictation_from(app.clone(), OperationSource::Hotkey)
            {
                events::emit_error(app, events::ErrorCodeV1::Audio, e.to_string(), None);
                tracing::warn!("start voice command recording failed: {e}");
            }
        }
        ShortcutState::Released => {
            if app.state::<pipeline::Pipeline>().is_recording() {
                let app_for_command = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = run_voice_command(&app_for_command).await {
                        tracing::warn!("voice command failed: {e}");
                        events::emit_error(
                            &app_for_command,
                            events::ErrorCodeV1::Internal,
                            e.to_string(),
                            None,
                        );
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
    let operation = pipeline.operation_id();
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or("active voice command has no cancellation signal")?;

    let samples = match pipeline.stop_recording() {
        Ok(samples) => samples,
        Err(error) => {
            let _ = pipeline::set_state_for_operation(
                app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(Box::new(error));
        }
    };
    if !pipeline.is_operation_active(operation) {
        tracing::info!("voice command discarded because dictation was cancelled or replaced");
        return Ok(());
    }
    if samples.is_empty() {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(());
    }

    let samples = crate::vad::trim_silence(&samples);
    if samples.is_empty() {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Completed,
        );
        return Ok(());
    }

    // Загружаем основную whisper-модель.
    if let Some(path) = settings.whisper_model_path.as_deref() {
        let stt = pipeline.stt().clone();
        let path = std::path::PathBuf::from(path);
        let acceleration = settings.acceleration;
        let worker_paths = stt::worker_paths_for_app(app);
        let load = tauri::async_runtime::spawn_blocking(move || {
            stt.ensure_loaded(&path, acceleration, &worker_paths)
        });
        let load_result = tokio::select! {
            result = load => Some(result),
            _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
        };
        let Some(load_result) = load_result else {
            return Ok(());
        };
        if let Err(error) = load_result.map_err(|error| format!("model load join: {error}"))? {
            let _ = pipeline::set_state_for_operation(
                app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(Box::new(error));
        }
    } else {
        let _ = pipeline::set_state_for_operation(
            app,
            state.inner(),
            &pipeline,
            operation,
            PipelineState::Idle,
            TerminalReason::Failed,
        );
        return Err("Whisper-модель не выбрана".into());
    }

    if !pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Transcribing,
        TerminalReason::Completed,
    ) {
        return Ok(());
    }
    let stt = pipeline.stt().clone();
    let language = settings.language.clone();
    let stt_cancellation = cancellation.clone();
    let transcribe = tauri::async_runtime::spawn_blocking(move || {
        stt.transcribe_cancellable(&samples, &language, stt_cancellation)
    });
    let transcript = tokio::select! {
        result = transcribe => Some(result),
        _ = application::dictation::wait_for_cancellation(cancellation) => None,
    };
    let Some(transcript) = transcript else {
        return Ok(());
    };
    let transcript = transcript.map_err(|e| format!("transcribe join: {e}"))??;

    if !pipeline.is_operation_active(operation) {
        tracing::info!(
            "voice command transcript discarded because dictation was cancelled or replaced"
        );
        return Ok(());
    }

    tracing::info!(
        "voice command transcript ready ({} chars)",
        transcript.text.chars().count()
    );

    let proposal = application::command_proposal::create(
        state.inner(),
        operation,
        OperationSource::Hotkey,
        transcript.text.clone(),
        None,
    );
    tracing::info!(
        proposal_id = proposal.id,
        operation = proposal.operation_id,
        expires_at = %proposal.expires_at,
        "voice command proposal created"
    );
    let _ = app.emit("command-proposal", transcript.text);
    if let Some(settings_window) = app.get_webview_window("settings") {
        let _ = settings_window.unminimize();
        let _ = settings_window.show();
        let _ = settings_window.set_focus();
    }
    let _ = pipeline::set_state_for_operation(
        app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );
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
                                    app,
                                    state.inner(),
                                    crate::types::PipelineState::Idle,
                                );
                            }
                            Err(e) => {
                                events::emit_error(
                                    app,
                                    events::ErrorCodeV1::Audio,
                                    e.to_string(),
                                    None,
                                );
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
                        ipc::wake::disable_wake_word(app_clone.clone()).await
                    } else {
                        ipc::wake::enable_wake_word(app_clone.clone()).await
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
                            events::emit_error(
                                &app_clone,
                                events::ErrorCodeV1::Wake,
                                e.to_string(),
                                None,
                            );
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
    let model_dir = match settings.wake_backend {
        WakeWordBackend::SherpaOnnx => state::app_data_dir()?
            .join("kws-models")
            .join("sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01"),
        WakeWordBackend::WhisperExperimental => {
            state::models_dir()?.join(settings.wake_word_model.filename())
        }
        _ => std::path::PathBuf::new(),
    };
    Ok(WakeWordConfig {
        enabled: settings.wake_word_enabled,
        backend: settings.wake_backend,
        phrase: settings.wake_word.clone(),
        audio_device_id: settings.audio_device_id.clone(),
        sample_rate: 16_000,
        threshold: settings.wake_word_threshold,
        sensitivity: settings.wake_word_sensitivity,
        vad_threshold: settings.wake_word_vad_threshold,
        use_gpu: settings.acceleration.use_gpu(),
        cooldown_ms: 2_000,
        model_dir,
    })
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
        WakeWordEvent::Detected { phrase, pre_roll } => {
            tracing::info!("wake word detected: {phrase}");
            events::emit_wake_detected(&handle_clone, &phrase);
            let h = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = run_dictation_after_wake(&h, pre_roll).await {
                    tracing::error!("dictation after wake failed: {e:?}");
                    events::emit_error(&h, events::ErrorCodeV1::Wake, e.to_string(), None);
                }
            });
        }
        WakeWordEvent::Error { message } => {
            tracing::error!("wake word error: {message}");
            events::emit_error(&handle_clone, events::ErrorCodeV1::Wake, &message, None);
        }
        WakeWordEvent::Listening => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Listening);
        }
        WakeWordEvent::Paused => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Paused);
        }
        WakeWordEvent::ModelLoading => {
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::Loading);
        }
        WakeWordEvent::MissingModel { path } => {
            tracing::warn!("wake word model missing: {path}");
            events::emit_wake_status(&handle_clone, events::WakeStatusV1::MissingModel);
            events::emit_error(
                &handle_clone,
                events::ErrorCodeV1::Wake,
                format!("Wake word: модель не найдена. Скачайте её в настройках: {path}"),
                None,
            );
        }
    });

    // Подготавливаем основную модель до того, как detector начнёт принимать
    // ключевую фразу. Раньше первая диктовка после запуска попадала в STT без
    // контекста модели и оставляла wake word на паузе.
    if let Some(path) = settings.whisper_model_path.clone() {
        let stt = handle.state::<pipeline::Pipeline>().stt().clone();
        let acceleration = settings.acceleration;
        let worker_paths = stt::worker_paths_for_app(handle);
        events::emit_wake_status(handle, events::WakeStatusV1::Loading);
        tauri::async_runtime::spawn_blocking(move || {
            stt.ensure_loaded(std::path::Path::new(&path), acceleration, &worker_paths)
        })
        .await
        .map_err(|e| format!("primary whisper load join: {e}"))??;
    } else {
        return Err("Whisper-модель для диктовки не выбрана".into());
    }

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
    pre_roll: Vec<i16>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let pipeline = handle.state::<pipeline::Pipeline>();
    let wake_handle = handle.state::<WakeWordHandle>();
    let settings = state.settings();

    // The listener is always resumed by the guard, including error paths.
    let _wake_pause = WakePauseGuard::pause(wake_handle.inner());

    // Стартуем запись.
    if let Err(error) = pipeline.start_recording_with_pre_roll_from(
        settings.audio_device_id.as_deref(),
        &pre_roll,
        OperationSource::WakeWord,
    ) {
        return Err(Box::new(error));
    }
    let operation = pipeline.operation_id();
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or("active wake dictation has no cancellation signal")?;
    if !pipeline::set_state_for_operation(
        handle,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Listening,
        TerminalReason::Completed,
    ) {
        return Ok(());
    }
    events::emit_pipeline_mode(handle, events::PipelineModeV1::Dictation);
    tracing::info!("wake dictation: recording started, waiting for VAD silence");

    // Ждём окончания речи: ловим начало речи, затем остановку по тишине.
    // Уровень звука читаем из writer-буфера записи (см. Pipeline::current_level).
    let max_wait = std::time::Duration::from_secs(30); // максимум 30 сек диктовки
                                                       // У тихой речи RMS может быть ниже прежнего жёсткого 0.012. Порог
                                                       // настраивается отдельно от VAD wake word и имеет гистерезис: после начала
                                                       // речи используем более низкий порог удержания, чтобы короткие тихие слоги
                                                       // не запускали обратный отсчёт посреди фразы.
    let speech_threshold = settings.wake_dictation_speech_threshold.clamp(0.002, 0.03);
    let sustain_threshold = (speech_threshold * 0.65).max(0.0015);
    let silence_timeout =
        std::time::Duration::from_millis(settings.wake_dictation_silence_ms.clamp(500, 10_000));
    let started = std::time::Instant::now();
    let mut was_speaking = false;
    let mut silence_start: Option<std::time::Instant> = None;
    // Показываем оверлей сразу: раньше он появлялся только после первого
    // VAD-сэмпла выше порога, из-за чего казалось, что он срабатывает не всегда.
    emit_wake_dictation_countdown(handle, silence_timeout, silence_timeout, true);

    loop {
        if !pipeline.is_operation_active(operation) {
            return Ok(());
        }
        if started.elapsed() >= max_wait || pipeline.is_operation_confirmed(operation) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let level = pipeline.current_level();
        let active_threshold = if was_speaking {
            sustain_threshold
        } else {
            speech_threshold
        };
        if level > active_threshold {
            // Идёт речь — сбрасываем счётчик тишины.
            // Отправляем событие только при возвращении к речи: прежняя версия
            // слала его каждые 100 мс, перегружая WebView и визуально "замораживая"
            // шкалу. Частые обновления нужны только настоящему отсчёту тишины.
            let resumed_speaking = silence_start.is_some();
            was_speaking = true;
            silence_start = None;
            if resumed_speaking {
                emit_wake_dictation_countdown(handle, silence_timeout, silence_timeout, true);
            }
        } else if was_speaking && silence_start.is_none() {
            // Речь была, началась тишина — запускаем таймер.
            silence_start = Some(std::time::Instant::now());
            tracing::debug!("wake dictation: silence started (level={:.4})", level);
        }

        // Тишина длится дольше порога после речи → останавливаем запись.
        if let Some(s) = silence_start {
            let elapsed = s.elapsed();
            let remaining = silence_timeout.saturating_sub(elapsed);
            emit_wake_dictation_countdown(handle, remaining, silence_timeout, false);

            if elapsed >= silence_timeout {
                tracing::info!(
                    "wake dictation: silence {:.1}s reached, stopping",
                    elapsed.as_secs_f32()
                );
                break;
            }
        }
    }

    emit_wake_dictation_countdown(handle, std::time::Duration::ZERO, silence_timeout, false);

    tracing::info!(
        "wake dictation: stopping recording after {:.1}s (was_speaking={})",
        started.elapsed().as_secs_f32(),
        was_speaking
    );

    // Стоп + STT + вставка.
    let samples = match pipeline.stop_recording() {
        Ok(samples) => samples,
        Err(error) => {
            let _ = pipeline::set_state_for_operation(
                handle,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(Box::new(error));
        }
    };

    // Если пользователь нажал Stop в оверлее — отбрасываем запись.
    if !pipeline.is_operation_active(operation) {
        tracing::info!("wake dictation: cancelled by user");
        return Ok(());
    }

    if !samples.is_empty() {
        // Запись уже завершается по VAD выше. Не применяем второй агрессивный
        // trim_silence: у него был фиксированный порог 1.2%, который отрезал
        // тихие первые и последние слова после wake word.
        if !samples.is_empty() {
            if let Some(path) = settings.whisper_model_path.as_deref() {
                let stt = pipeline.stt().clone();
                let path = std::path::PathBuf::from(path);
                let acceleration = settings.acceleration;
                let worker_paths = stt::worker_paths_for_app(handle);
                let load = tauri::async_runtime::spawn_blocking(move || {
                    stt.ensure_loaded(&path, acceleration, &worker_paths)
                });
                let load_result = tokio::select! {
                    result = load => Some(result),
                    _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
                };
                let Some(load_result) = load_result else {
                    return Ok(());
                };
                if let Err(error) =
                    load_result.map_err(|error| format!("model load join: {error}"))?
                {
                    let _ = pipeline::set_state_for_operation(
                        handle,
                        state.inner(),
                        &pipeline,
                        operation,
                        PipelineState::Idle,
                        TerminalReason::Failed,
                    );
                    return Err(Box::new(error));
                }
            } else {
                let _ = pipeline::set_state_for_operation(
                    handle,
                    state.inner(),
                    &pipeline,
                    operation,
                    PipelineState::Idle,
                    TerminalReason::Failed,
                );
                return Err("Whisper-модель для диктовки не выбрана".into());
            }
            if !pipeline::set_state_for_operation(
                handle,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Transcribing,
                TerminalReason::Completed,
            ) {
                return Ok(());
            }
            let stt = pipeline.stt().clone();
            let language = settings.language.clone();
            let stt_cancellation = cancellation.clone();
            let transcribe = tauri::async_runtime::spawn_blocking(move || {
                stt.transcribe_cancellable(&samples, &language, stt_cancellation)
            });
            let transcript = tokio::select! {
                result = transcribe => Some(result),
                _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
            };
            let Some(transcript) = transcript else {
                return Ok(());
            };
            let transcript = match transcript {
                Ok(Ok(transcript)) => transcript,
                Ok(Err(error)) => {
                    let _ = pipeline::set_state_for_operation(
                        handle,
                        state.inner(),
                        &pipeline,
                        operation,
                        PipelineState::Idle,
                        TerminalReason::Failed,
                    );
                    return Err(Box::new(error));
                }
                Err(error) => {
                    let _ = pipeline::set_state_for_operation(
                        handle,
                        state.inner(),
                        &pipeline,
                        operation,
                        PipelineState::Idle,
                        TerminalReason::Failed,
                    );
                    return Err(format!("transcribe join: {error}").into());
                }
            };

            if !pipeline.is_operation_active(operation) {
                tracing::info!("wake dictation transcript discarded because operation was cancelled or replaced");
                return Ok(());
            }

            let dictation_text = strip_leading_wake_phrase(&transcript.text, &settings.wake_word);
            if dictation_text.len() != transcript.text.trim_start().len() {
                tracing::info!("wake phrase removed from dictation transcript");
            }
            tracing::info!(
                "wake dictation transcript ready ({} chars)",
                dictation_text.chars().count()
            );

            // Явная команда после wake phrase не вставляется в активное окно
            // и не выполняется автоматически. Например: «okay fun, команда,
            // открой Telegram» или «okay fun, команда, громче». Она проходит
            // тот же preview → confirm flow, что и command hotkey.
            if let Some(command) = extract_wake_command(&dictation_text) {
                if !pipeline.is_operation_active(operation) {
                    return Ok(());
                }
                if !pipeline::set_state_for_operation(
                    handle,
                    state.inner(),
                    &pipeline,
                    operation,
                    PipelineState::Processing,
                    TerminalReason::Completed,
                ) {
                    return Ok(());
                }
                let proposal = application::command_proposal::create(
                    state.inner(),
                    operation,
                    OperationSource::WakeWord,
                    command,
                    None,
                );
                tracing::info!(
                    proposal_id = proposal.id,
                    operation = proposal.operation_id,
                    expires_at = %proposal.expires_at,
                    "wake command proposal created"
                );
                let _ = handle.emit("command-proposal", proposal.original_text);
                if let Some(settings_window) = handle.get_webview_window("settings") {
                    let _ = settings_window.unminimize();
                    let _ = settings_window.show();
                    let _ = settings_window.set_focus();
                }
                let _ = pipeline::set_state_for_operation(
                    handle,
                    state.inner(),
                    &pipeline,
                    operation,
                    PipelineState::Idle,
                    TerminalReason::Completed,
                );
                return Ok(());
            }

            // Опциональная AI-обработка.
            let final_text = match settings.ai_mode {
                crate::types::AiMode::Off => dictation_text.clone(),
                mode => {
                    if !pipeline::set_state_for_operation(
                        handle,
                        state.inner(),
                        &pipeline,
                        operation,
                        PipelineState::Processing,
                        TerminalReason::Completed,
                    ) {
                        return Ok(());
                    }
                    let client = crate::llm::LlmClient::from_settings(&settings);
                    let process =
                        client.process(&dictation_text, mode, settings.clean_prompt.as_deref());
                    let result = tokio::select! {
                        result = process => Some(result),
                        _ = application::dictation::wait_for_cancellation(cancellation.clone()) => None,
                    };
                    let Some(result) = result else {
                        return Ok(());
                    };
                    match result {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!("LLM failed ({e}) — raw transcript");
                            dictation_text.clone()
                        }
                    }
                }
            };

            if !pipeline.is_operation_active(operation) {
                tracing::info!(
                    "wake dictation result discarded because operation was cancelled or replaced"
                );
                return Ok(());
            }

            if !final_text.is_empty() {
                if !pipeline::set_state_for_operation(
                    handle,
                    state.inner(),
                    &pipeline,
                    operation,
                    PipelineState::Injecting,
                    TerminalReason::Completed,
                ) {
                    return Ok(());
                }
                if let Err(e) = crate::injection::inject_text(&final_text, settings.injection_mode)
                {
                    tracing::warn!("injection failed: {e}");
                    events::emit_error(
                        handle,
                        events::ErrorCodeV1::Injection,
                        format!("Вставка: {e}"),
                        Some(operation),
                    );
                }
            }
        }
    }

    let _ = pipeline::set_state_for_operation(
        handle,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );

    // Резюммим wake word.

    Ok(())
}

#[cfg(test)]
mod wake_command_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{extract_wake_command, strip_leading_wake_phrase, WakeLifecycle, WakePauseGuard};

    #[derive(Default)]
    struct FakeWakeLifecycle {
        pauses: AtomicUsize,
        resumes: AtomicUsize,
    }

    impl WakeLifecycle for FakeWakeLifecycle {
        fn pause(&self) {
            self.pauses.fetch_add(1, Ordering::SeqCst);
        }

        fn resume(&self) {
            self.resumes.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn wake_pause_guard_resumes_on_early_scope_exit() {
        let wake = FakeWakeLifecycle::default();
        {
            let _guard = WakePauseGuard::pause(&wake);
            assert_eq!(wake.pauses.load(Ordering::SeqCst), 1);
            assert_eq!(wake.resumes.load(Ordering::SeqCst), 0);
        }
        assert_eq!(wake.resumes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn extracts_only_explicit_command_prefixes() {
        assert_eq!(
            extract_wake_command("Команда: открой Telegram"),
            Some("открой telegram".to_string())
        );
        assert_eq!(
            extract_wake_command("command louder"),
            Some("louder".to_string())
        );
        assert_eq!(extract_wake_command("открой Telegram"), None);
    }

    #[test]
    fn removes_only_leading_wake_phrase_from_dictation() {
        assert_eq!(
            strip_leading_wake_phrase("okay fun, напиши привет", "okay fun"),
            "напиши привет"
        );
        assert_eq!(
            strip_leading_wake_phrase("ok fan open telegram", "okay fun"),
            "open telegram"
        );
        assert_eq!(
            strip_leading_wake_phrase("просто okay fun внутри фразы", "okay fun"),
            "просто okay fun внутри фразы"
        );
    }
}
