//! Fono — голосовой ввод для Windows.
//!
//! Архитектуру и потоки данных см. в `docs/architecture.md`.

pub mod app_commands;
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

use crate::state::AppState;
use crate::types::PipelineState;
#[cfg(feature = "wake-word")]
use crate::types::Settings;
use fono_audio::AudioHub;
#[cfg(feature = "wake-word")]
use fono_wake::{WakeWordConfig, WakeWordEvent, WakeWordHandle};
use tauri::{Manager, RunEvent, WindowEvent};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Возвращает действие, явно продиктованное после wake phrase.
///
/// Намеренно не пытаемся угадывать команды из обычного текста: системные
/// действия должны требовать явного префикса «команда». Результат в нижнем
/// регистре безопасен для текущего сопоставления окон и приложений.
#[cfg(feature = "wake-word")]
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
#[cfg(feature = "wake-word")]
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
    let expected: Vec<String> = configured_phrase
        .split_whitespace()
        .map(normalize_word)
        .collect();
    let matched = expected
        .iter()
        .zip(&words)
        .all(|(expected, actual)| !expected.is_empty() && expected == &normalize_word(actual));
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
        .build(&log_dir);
    let Ok(file_appender) = file_appender else {
        // A read-only/full profile must not prevent dictation from starting.
        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_target(false).with_file(false))
            .try_init();
        tracing::warn!("file logging unavailable; using console logging");
        return;
    };
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
    crash_log::install(|| state::app_data_dir().ok().map(|dir| dir.join("logs")));
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
        .setup(|app| {
            // Only the primary process may load settings and own services.
            init_tracing();
            tracing::info!("Fono starting up…");
            let audio_hub = AudioHub::new();
            app.manage(state::AppState::new());
            app.manage(crate::application::updates::UpdateService::default());
            app.manage(crate::application::gpu_memory::SttMemoryRuntime::default());
            app.manage(pipeline::Pipeline::new_with_audio_hub(audio_hub.clone()));
            #[cfg(feature = "experimental-live")]
            app.manage(crate::application::live_dictation::LiveController::default());
            app.manage(crate::application::dictation::workflow::Runtime::default());
            app.manage(crate::application::dictation::ProcessingCaptureRuntime::default());
            #[cfg(feature = "wake-word")]
            app.manage(WakeWordHandle::new_with_audio_hub(
                WakeWordConfig::default(),
                audio_hub.clone(),
            ));
            app.manage(audio_hub);
            app.manage(crate::overlay::OverlayRuntime::default());
            app.manage(ipc::desktop_v3::DesktopSession::default());
            app.manage(crate::application::service_control::ServiceControl::default());
            #[cfg(feature = "wake-word")]
            app.manage(crate::application::wake_calibration::WakeCalibrationService::default());
            #[cfg(feature = "wake-word")]
            app.manage(
                crate::application::wake_validation::WakeProfileValidationService::default(),
            );
            #[cfg(windows)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            // Трей-иконка с меню
            setup_tray(app)?;
            // Настраиваемые горячие клавиши диктовки: hold или toggle.
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

            // Применяем сохранённые настройки и политику хранения аналитики.
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
            crate::application::gpu_memory::start(app.handle().clone());
            app.manage(
                crate::application::speech_analysis_queue::SpeechAnalysisQueue::start(
                    app.handle().clone(),
                )?,
            );
            if let Err(error) = crate::overlay::restore_position(app.handle(), &settings) {
                tracing::warn!(%error, "could not restore overlay position");
            }

            #[cfg(feature = "wake-word")]
            {
                // Сервис пробуждения проверяет доступность функции и её настройки.
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = start_wake_word_if_enabled(&handle).await {
                        tracing::error!("wake word start failed: {e:?}");
                    }
                });
            }

            crate::application::updates::check_on_startup(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // state
            ipc::desktop_v3::get_desktop_snapshot,
            ipc::memory::get_stt_memory_status,
            ipc::desktop_v3::improve_text,
            ipc::desktop_v3::clear_speech_analytics,
            ipc::desktop_v3::recommend_speech,
            ipc::desktop_v3::get_microphone_sample,
            ipc::desktop_v3::test_microphone_device,
            ipc::desktop_v3::remove_whisper_model,
            ipc::desktop_v3::get_selected_model_metadata,
            ipc::system::get_build_info,
            #[cfg(feature = "experimental-live")]
            ipc::dictation::get_live_dictation,
            #[cfg(feature = "experimental-live")]
            ipc::dictation::resume_live_insertion,
            ipc::dictation::start_dictation,
            ipc::dictation::stop_dictation,
            ipc::dictation::finish_overlay_dictation,
            ipc::dictation::acknowledge_overlay_processing_flush,
            ipc::dictation::confirm_dictation,
            ipc::dictation::cancel_dictation,
            ipc::dictation::dismiss_dictation_overlay,
            ipc::dictation::transcribe_test,
            ipc::dictation::get_pending_dictation,
            ipc::dictation::resolve_pending_dictation,
            ipc::dictation::update_overlay_processing_choice,
            ipc::system::get_dictation_history,
            ipc::system::clear_dictation_history,
            ipc::system::delete_dictation_history_entry,
            ipc::system::set_dictation_history_entry_analytics_included,
            ipc::system::get_speech_session_analysis,
            ipc::system::get_speech_period_report,
            ipc::trainer::get_trainer_report,
            ipc::trainer::analyze_trainer_period,
            ipc::trainer::cancel_trainer_period,
            ipc::service::get_local_transcription_service_snapshot,
            ipc::service::set_local_transcription_service_enabled,
            ipc::service::cancel_local_transcription_job,
            ipc::service::clear_local_transcription_history,
            ipc::service::copy_local_transcription_api_token,
            ipc::text::copy_dictation_text,
            ipc::text::reinsert_dictation,
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
            #[cfg(feature = "wake-word")]
            ipc::models::is_kws_model_downloaded,
            #[cfg(feature = "wake-word")]
            ipc::models::download_kws_model,
            ipc::models::cancel_model_download,
            // llm
            ipc::llm::test_llm_profile,
            ipc::llm::list_llm_profile_models,
            ipc::processing::get_processing_prompt_catalog,
            ipc::processing::preview_processing_text,
            ipc::processing_capture::start_processing_test_capture,
            ipc::processing_capture::finish_processing_test_capture,
            ipc::processing_capture::cancel_processing_test_capture,
            // settings
            ipc::settings::get_settings,
            ipc::settings::get_acceleration_capabilities,
            ipc::settings::get_stt_readiness,
            ipc::settings::get_stt_health,
            ipc::settings::save_settings,
            // overlay
            ipc::settings::save_overlay_position,
            overlay::get_overlay_preview,
            overlay::set_overlay_layout,
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
            ipc::project::open_project_site,
            ipc::project::open_feedback_form,
            ipc::diagnostics::clear_logs,
            ipc::diagnostics::test_microphone,
            // wake word
            #[cfg(feature = "wake-word")]
            ipc::wake::get_wake_word_status,
            #[cfg(feature = "wake-word")]
            ipc::wake::get_wake_word_capabilities,
            #[cfg(feature = "wake-word")]
            ipc::wake::get_wake_word_diagnostics,
            #[cfg(feature = "wake-word")]
            ipc::wake::test_wake_word_model,
            #[cfg(feature = "wake-word")]
            ipc::wake::record_wake_word_sample,
            #[cfg(feature = "wake-word")]
            ipc::wake::recognize_wake_word_sample,
            #[cfg(feature = "wake-word")]
            ipc::wake::get_wake_calibration_status,
            #[cfg(feature = "wake-word")]
            ipc::wake::start_wake_calibration,
            #[cfg(feature = "wake-word")]
            ipc::wake::record_wake_calibration_sample,
            #[cfg(feature = "wake-word")]
            ipc::wake::cancel_wake_calibration,
            #[cfg(feature = "wake-word")]
            ipc::wake::get_wake_profile_validation_status,
            #[cfg(feature = "wake-word")]
            ipc::wake::start_wake_profile_validation,
            #[cfg(feature = "wake-word")]
            ipc::wake::record_wake_profile_validation_sample,
            #[cfg(feature = "wake-word")]
            ipc::wake::enable_wake_word,
            #[cfg(feature = "wake-word")]
            ipc::wake::disable_wake_word,
        ])
        .build(context);
    drop(startup_gate);
    let Ok(app) = app else {
        tracing::error!("Не удалось запустить Fono. Проверьте доступ к профилю приложения.");
        return;
    };
    app.run(|app, event| {
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
    if let Some(pipeline) = app.try_state::<pipeline::Pipeline>() {
        if let Some(event) = pipeline.shutdown() {
            events::emit_operation(app, event);
        }
    }
    injection::shutdown();
    #[cfg(feature = "wake-word")]
    if let Some(wake) = app.try_state::<WakeWordHandle>() {
        wake.stop();
    }
    if let Some(audio) = app.try_state::<AudioHub>() {
        audio.shutdown();
    }
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    if let Some(state) = app.try_state::<AppState>() {
        pipeline::set_state(app, state.inner(), PipelineState::Idle);
    }
}

/// Регистрирует глобальные горячие клавиши:
/// - push-to-talk (dictation)
/// - voice commands
fn setup_global_shortcut(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();
    let settings = app_handle.state::<AppState>().settings();
    if let Err(e) = application::hotkeys::register_all_shortcuts(&app_handle, &settings) {
        tracing::error!("Не удалось зарегистрировать горячие клавиши: {e}");
        tracing::error!("Возможно, одна из клавиш уже занята другим приложением.");
    }
    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    crate::tray::setup(app)
}

/// Builds a backend-specific wake word config from application settings.
#[cfg(feature = "wake-word")]
fn settings_to_wake_config(settings: &Settings) -> crate::error::AppResult<WakeWordConfig> {
    let model_dir = fono_wake::model_spec(settings.wake_backend)
        .map(|spec| state::app_data_dir().map(|dir| dir.join("kws-models").join(spec.directory)))
        .transpose()?
        .unwrap_or_default();
    Ok(WakeWordConfig {
        enabled: types::WAKE_WORD_AVAILABLE && settings.wake_word_enabled,
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
/// Использует отдельно выбранный исследовательский streaming backend.
/// При обнаружении wake-фразы стартует запись диктовки,
/// по тишине (VAD) — STT (основной моделью) + вставка текста.
#[cfg(feature = "wake-word")]
async fn start_wake_word_if_enabled(
    handle: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    if !types::WAKE_WORD_AVAILABLE {
        tracing::info!("wake word: temporarily unavailable, skipping");
        return Ok(());
    }
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
#[cfg(feature = "wake-word")]
pub async fn restart_wake_word(handle: &tauri::AppHandle) -> Result<(), String> {
    let wake_handle = handle.state::<WakeWordHandle>();
    let settings = handle.state::<state::AppState>().settings();
    let config = settings_to_wake_config(&settings).map_err(|e| e.to_string())?;
    wake_handle.update_config(config).map_err(|e| e.to_string())
}

#[cfg(all(test, feature = "wake-word"))]
mod wake_command_tests {
    use super::{extract_wake_command, strip_leading_wake_phrase};

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
            strip_leading_wake_phrase("ЭЙ ФОНО, напиши привет", "Эй Фоно"),
            "напиши привет"
        );
        assert_eq!(
            strip_leading_wake_phrase("эй фон open telegram", "Эй Фоно"),
            "эй фон open telegram"
        );
        assert_eq!(
            strip_leading_wake_phrase("просто Эй Фоно внутри фразы", "Эй Фоно"),
            "просто Эй Фоно внутри фразы"
        );
    }
}
