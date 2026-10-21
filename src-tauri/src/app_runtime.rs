//! Native service composition and orderly process shutdown.

use crate::state::AppState;
use crate::types::PipelineState;
use crate::{application, events, injection, ipc, pipeline, state};
use fono_audio::AudioHub;
#[cfg(feature = "wake-word")]
use fono_wake::{WakeWordConfig, WakeWordHandle};
use tauri::{Manager, WindowEvent};

pub(crate) struct RuntimeReady;

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Only the primary process may load settings and own services.
    crate::logging::init();
    tracing::info!("Fono starting up…");
    let audio_hub = AudioHub::new();
    app.manage(state::AppState::new());
    app.manage(crate::application::recording_sounds::Runtime::default());
    app.manage(crate::application::updates::UpdateService::default());
    app.manage(crate::application::cuda_packages::CudaPackageService::new(
        crate::application::cuda_packages::package_root()?,
    ));
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
    app.manage(crate::cli::Runtime::default());
    app.manage(crate::application::hotkeys::HookRuntime::default());
    app.manage(crate::application::hotkeys::capture::Runtime::default());
    app.manage(crate::application::selection_capture::Runtime::default());
    app.manage(crate::application::service_control::ServiceControl::default());
    #[cfg(feature = "wake-word")]
    app.manage(crate::application::wake_calibration::WakeCalibrationService::default());
    #[cfg(feature = "wake-word")]
    app.manage(crate::application::wake_validation::WakeProfileValidationService::default());
    #[cfg(windows)]
    app.handle()
        .plugin(tauri_plugin_updater::Builder::new().build())?;
    // Трей-иконка с меню
    crate::tray::setup(app)?;
    // Настраиваемые горячие клавиши диктовки: hold или toggle.
    setup_global_shortcut(app)?;

    // Применяем сохранённые настройки и политику хранения аналитики.
    let settings = app.state::<AppState>().settings();
    crate::verbose::set_verbose(settings.verbose_logging);
    let repository = std::sync::Arc::new(crate::history::HistoryRepository::new(
        crate::application::history_events::hooks(),
    ));
    let history_events = app.handle().clone();
    repository.set_change_observer(std::sync::Arc::new(move |_| {
        events::emit_history_changed(&history_events);
    }));
    app.manage(repository.clone());
    if let Err(error) = repository.maintain(&settings) {
        tracing::warn!(%error, "could not apply analytics privacy policy at startup");
    }
    crate::application::history_events::start_maintenance(app.handle().clone());
    let readiness_events = app.handle().clone();
    app.state::<pipeline::Pipeline>()
        .stt()
        .set_readiness_observer(std::sync::Arc::new(move |readiness| {
            events::emit_stt_readiness(&readiness_events, readiness);
        }));
    app.state::<crate::application::service_control::ServiceControl>()
        .initialize(app.handle().clone());
    crate::application::gpu_memory::start(app.handle().clone());
    app.manage(
        crate::application::speech_analysis_queue::SpeechAnalysisQueue::start(
            app.handle().clone(),
        )?,
    );

    #[cfg(feature = "wake-word")]
    {
        // Сервис пробуждения проверяет доступность функции и её настройки.
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::application::wake::start_if_enabled(&handle).await {
                tracing::error!("wake word start failed: {e:?}");
            }
        });
    }

    crate::application::updates::check_on_startup(app.handle().clone());
    let cuda_app = app.handle().clone();
    if let Ok(activity) = crate::application::updates::activity::lease() {
        tauri::async_runtime::spawn_blocking(move || {
            let _activity = activity;
            if cuda_app
                .state::<crate::application::cuda_packages::CudaPackageService>()
                .refresh_installed()
                .is_err()
            {
                tracing::warn!("could not verify an installed CUDA package");
            }
            crate::application::models::preload_configured_stt(cuda_app);
        });
    }

    // Tauri normally creates configured windows before setup. WebView2 can
    // execute their scripts during creation, so all IPC state must exist first.
    // Both configs use create:false; preserve their settings when building here.
    app.manage(RuntimeReady);
    for config in &app.config().app.windows {
        tauri::WebviewWindowBuilder::from_config(app.handle(), config)?.build()?;
    }
    if let Some(window) = app.get_webview_window("settings") {
        let settings_window = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if let Some(snapshot) = settings_window
                    .app_handle()
                    .state::<crate::application::selection_capture::Runtime>()
                    .snapshot()
                {
                    crate::application::selection_capture::cancel(
                        settings_window.app_handle(),
                        snapshot.operation_id,
                    );
                }
                api.prevent_close();
                let _ = settings_window.hide();
                tracing::info!("settings window hidden to tray");
            }
        });
    }
    if let Err(error) = crate::overlay::restore_position(app.handle(), &settings) {
        tracing::warn!(%error, "could not restore overlay position");
    }
    let _ = tauri::Emitter::emit(app, "fono-runtime-ready", true);
    Ok(())
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    if let Some(selection) = app.try_state::<crate::application::selection_capture::Runtime>() {
        selection.close();
    }
    if let Some(capture) = app.try_state::<crate::application::hotkeys::capture::Runtime>() {
        capture.close();
    }
    tracing::info!("Fono shutdown requested");
    if let Some(cli) = app.try_state::<crate::cli::Runtime>() {
        cli.shutdown();
    }
    // Stop input admission and unhook/join its owning thread before capture teardown.
    if let Some(hooks) = app.try_state::<crate::application::hotkeys::HookRuntime>() {
        hooks.stop();
    }
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
