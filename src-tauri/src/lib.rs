//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
pub mod application;
pub mod audio;
mod autostart;
pub mod error;
pub mod events;
pub mod history;
pub mod injection;
pub mod ipc;
pub mod llm;
pub mod operation;
pub mod overlay;
mod overlay_geometry;
pub mod pipeline;
pub mod secrets;
pub mod service_history;
pub mod speech_metrics;
pub mod state;
pub mod stt;
pub mod types;
pub mod vad;
pub mod verbose;

use crate::operation::OperationSource;
use crate::state::AppState;
use crate::types::{PipelineState, Settings, WakeWordBackend};
use fono_wake::{AudioHub, WakeWordConfig, WakeWordEvent, WakeWordHandle};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Restores wake-word listening when a wake-triggered operation leaves scope.
/// The guard deliberately owns no operation state: it only pairs the engine
/// pause with its mandatory resume across every early return and await point.
#[cfg(test)]
trait WakeLifecycle {
    fn pause(&self);
    fn resume(&self);
}

#[cfg(test)]
impl WakeLifecycle for WakeWordHandle {
    fn pause(&self) {
        WakeWordHandle::pause(self);
    }

    fn resume(&self) {
        WakeWordHandle::resume(self);
    }
}

#[cfg(test)]
struct WakePauseGuard<'a, T: WakeLifecycle> {
    wake_handle: &'a T,
}

#[cfg(test)]
impl<'a, T: WakeLifecycle> WakePauseGuard<'a, T> {
    fn pause(wake_handle: &'a T) -> Self {
        wake_handle.pause();
        Self { wake_handle }
    }
}

#[cfg(test)]
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
    let count = configured_phrase.split_whitespace().count();
    let words: Vec<&str> = trimmed.split_whitespace().take(count).collect();
    if count == 0 || words.len() < count {
        return trimmed.to_string();
    }
    let normalize_word = |word: &str| {
        word.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let first = normalize_word(words[0]);
    let second = words
        .get(1)
        .map(|word| normalize_word(word))
        .unwrap_or_default();
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
            expected.len() == words.len()
                && expected
                    .iter()
                    .zip(&words)
                    .all(|(expected, actual)| expected == &normalize_word(actual))
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
        .manage(crate::application::updates::UpdateService::default())
        .manage(pipeline)
        .manage(crate::application::live_dictation::LiveController::default())
        .manage(wake_word)
        .manage(audio_hub)
        .manage(crate::overlay::OverlayRuntime::default())
        .manage(ipc::desktop_v3::DesktopSession::default())
        .manage(crate::application::service_control::ServiceControl::default())
        .manage(crate::application::wake_calibration::WakeCalibrationService::default())
        .manage(crate::application::wake_validation::WakeProfileValidationService::default())
        .setup(|app| {
            #[cfg(windows)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
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
            app.state::<crate::application::service_control::ServiceControl>()
                .initialize(app.handle().clone());
            app.manage(
                crate::application::speech_analysis_queue::SpeechAnalysisQueue::start(
                    app.handle().clone(),
                ),
            );
            if let Err(error) = crate::overlay::restore_position(app.handle(), &settings) {
                tracing::warn!(%error, "could not restore overlay position");
            }

            // Wake word: запускаем, если включён в настройках.
            // Загружает base-модель для транскрипции чанков (баланс скорости/точности).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_wake_word_if_enabled(&handle).await {
                    tracing::error!("wake word start failed: {e:?}");
                }
            });

            crate::application::updates::check_on_startup(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // state
            ipc::desktop_v3::get_desktop_snapshot,
            ipc::desktop_v3::improve_text,
            ipc::desktop_v3::clear_speech_analytics,
            ipc::desktop_v3::recommend_speech,
            ipc::desktop_v3::get_microphone_sample,
            ipc::desktop_v3::test_microphone_device,
            ipc::desktop_v3::remove_whisper_model,
            ipc::desktop_v3::get_selected_model_metadata,
            ipc::system::get_build_info,
            ipc::dictation::get_pipeline_state,
            ipc::dictation::get_live_dictation,
            ipc::dictation::resume_live_insertion,
            ipc::dictation::start_dictation,
            ipc::dictation::stop_dictation,
            ipc::dictation::confirm_dictation,
            ipc::dictation::cancel_dictation,
            ipc::dictation::transcribe_test,
            ipc::system::get_dictation_history,
            ipc::system::clear_dictation_history,
            ipc::system::delete_dictation_history_entry,
            ipc::system::set_dictation_history_entry_analytics_included,
            ipc::system::get_speech_session_analysis,
            ipc::system::get_speech_period_report,
            ipc::service::get_local_transcription_service_snapshot,
            ipc::service::set_local_transcription_service_enabled,
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
            overlay::get_overlay_preview,
            overlay::show_overlay_preview,
            overlay::reset_overlay_position,
            overlay::position_overlay,
            overlay::hide_overlay_preview,
            // диагностика
            ipc::diagnostics::get_recent_logs,
            ipc::diagnostic_report::get_diagnostic_report,
            ipc::updates::get_update_status,
            ipc::updates::check_for_updates,
            ipc::updates::install_update,
            ipc::updates::cancel_update_download,
            ipc::updates::set_update_checks_enabled,
            ipc::diagnostics::clear_logs,
            ipc::diagnostics::test_microphone,
            // wake word
            ipc::wake::get_wake_word_status,
            ipc::wake::get_wake_word_capabilities,
            ipc::wake::get_wake_word_diagnostics,
            ipc::wake::test_wake_word_model,
            ipc::wake::record_wake_word_sample,
            ipc::wake::recognize_wake_word_sample,
            ipc::wake::get_wake_calibration_status,
            ipc::wake::start_wake_calibration,
            ipc::wake::record_wake_calibration_sample,
            ipc::wake::cancel_wake_calibration,
            ipc::wake::get_wake_profile_validation_status,
            ipc::wake::start_wake_profile_validation,
            ipc::wake::record_wake_profile_validation_sample,
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
    // Exit can be requested while Tauri is still constructing its windows.
    // Cleanup must also tolerate an incomplete application setup.
    if let Some(service) = app.try_state::<crate::application::service_control::ServiceControl>() {
        service.shutdown();
    }
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
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let held_operation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    gs.on_shortcut(settings.hotkey.as_str(), move |app, _, event| {
        let live = application::live_dictation::is_active(app);
        match event.state {
            ShortcutState::Pressed => {
                if held.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    return;
                }
                if live {
                    let handle = app.clone();
                    let operation = app.state::<pipeline::Pipeline>().operation_id();
                    tauri::async_runtime::spawn(async move {
                        let _ = application::live_dictation::finish_for(handle, operation).await;
                    });
                    return;
                }
                events::emit_pipeline_mode(app, events::PipelineModeV1::Dictation);
                match ipc::dictation::start_dictation_from(app.clone(), OperationSource::Hotkey) {
                    Err(e) => {
                        events::emit_error(app, events::ErrorCodeV1::Audio, e.to_string(), None);
                        tracing::warn!("start_dictation via global shortcut failed: {e}");
                    }
                    Ok(operation) => {
                        held_operation.store(operation, std::sync::atomic::Ordering::Release);
                    }
                }
            }
            ShortcutState::Released => {
                held.store(false, std::sync::atomic::Ordering::Release);
                let operation = held_operation.swap(0, std::sync::atomic::Ordering::AcqRel);
                if live {
                    return;
                }
                if operation != 0 {
                    let app_for_stop = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = application::dictation::stop_with_reason_for(
                            app_for_stop,
                            operation,
                            application::dictation_tail_diagnostics::DictationStopReason::Manual,
                        )
                        .await
                        {
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
    let command_operation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    gs.on_shortcut(command_hotkey.as_str(), move |app, _, event| {
        match event.state {
            ShortcutState::Pressed => {
                events::emit_pipeline_mode(app, events::PipelineModeV1::Command);
                match application::dictation::start_command_operation(app.clone()) {
                    Err(e) => {
                        events::emit_error(app, events::ErrorCodeV1::Audio, e.to_string(), None);
                        tracing::warn!("start voice command recording failed: {e}");
                    }
                    Ok(operation) => {
                        command_operation.store(operation, std::sync::atomic::Ordering::Release);
                    }
                }
            }
            ShortcutState::Released => {
                let operation = command_operation.swap(0, std::sync::atomic::Ordering::AcqRel);
                if operation != 0 {
                    let app_for_command = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = run_voice_command(&app_for_command, operation).await {
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
        }
    })?;
    tracing::info!("command hotkey '{}' registered", command_hotkey);

    Ok(())
}

/// Полный цикл голосовой команды: запись → STT → выполнение.
async fn run_voice_command(
    app: &tauri::AppHandle,
    operation: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    application::voice_dictation::run(app, operation).await
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
                let is_paused = state.toggle_dictation_paused();
                if is_paused {
                    if let Err(error) = ipc::dictation::cancel_dictation(app.clone()) {
                        events::emit_error(
                            app,
                            events::ErrorCodeV1::Audio,
                            error.to_string(),
                            None,
                        );
                        tracing::warn!("pause cancellation failed: {error}");
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
        backend => fono_wake::model_spec(backend)
            .map(|spec| {
                state::app_data_dir().map(|dir| dir.join("kws-models").join(spec.directory))
            })
            .transpose()?
            .unwrap_or_default(),
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
        phrase_stability_ms: 150 + (settings.wake_word_threshold.clamp(0.0, 1.0) * 700.0) as u64,
        phrase_confirmations: 2,
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
    let activity = crate::application::updates::activity::lease()?;
    use tauri::Manager;
    let state = handle.state::<state::AppState>();
    let settings = state.settings();

    if !settings.wake_word_enabled {
        tracing::info!("wake word: disabled in settings, skipping");
        return Ok(());
    }
    crate::application::wake_validation::ensure_profile_can_activate(&settings)?;

    let wake_handle = handle.state::<WakeWordHandle>();
    let handle_clone = handle.clone();
    wake_handle.set_callback(move |event| match event {
        WakeWordEvent::Detected {
            phrase,
            pre_roll,
            audio_cursor,
        } => {
            tracing::info!("wake word detected: {phrase}");
            events::emit_wake_detected(&handle_clone, &phrase);
            let h = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = application::wake_dictation::run(&h, pre_roll, audio_cursor).await {
                    application::dictation::resume_wake_if_idle(&h);
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
        let background_activity = activity.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _activity = background_activity;
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
    application::wake_dictation::run(handle, pre_roll, None).await?;
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
