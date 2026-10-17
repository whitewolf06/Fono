//! Optional CUDA becomes visible only in an idle configuration transaction.
use crate::{
    application::{capture_configuration, cuda_packages::VerifiedPackage, updates},
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    stt::{self, RecoverySelection},
    types::AccelerationMode,
};
use tauri::{AppHandle, Manager};

pub(crate) fn activate(
    app: &AppHandle,
    package: &VerifiedPackage,
    commit: &mut dyn FnMut() -> AppResult<()>,
) -> AppResult<()> {
    // Called on the blocking installation thread, after all transfer/probe
    // work. Settings stay usable during the potentially large download.
    let _transaction = tauri::async_runtime::block_on(updates::settings_transaction(app));
    let _configuration = capture_configuration::reserve_configuration(|| ensure_idle(app))?;
    let state = app.state::<AppState>();
    let settings = state.settings();
    let old_paths = stt::worker_paths_for_app(app);
    let mut new_paths = old_paths.clone();
    new_paths.cuda = Some(package.worker_path().to_owned());
    new_paths.cuda_package = Some(package.clone());
    let stt = app.state::<Pipeline>().stt().clone();
    let selected = settings.whisper_model_path.as_deref().filter(|_| {
        matches!(
            settings.acceleration,
            AccelerationMode::Auto | AccelerationMode::Cuda
        )
    });
    let prepared = if let Some(path) = selected {
        stt.reset_recovery_budget();
        if let Err(error) = stt.ensure_loaded(
            std::path::Path::new(path),
            settings.acceleration,
            &new_paths,
        ) {
            // Even an unsuccessful prepare can have published a new engine
            // before discovering pending teardown of its previous worker.
            restore(&stt, path, settings.acceleration, &old_paths)?;
            return Err(error);
        }
        stt.prepared_recovery_selection(&RecoverySelection {
            model_path: path.into(),
            acceleration: settings.acceleration,
            worker_paths: new_paths,
        })
    } else {
        None
    };
    // This is the last fallible step. The service rechecks the operation's
    // cancellation under its own publication lock and atomically writes both
    // the pointer and high-water revision. Capture/API admission is still shut.
    if let Err(error) = commit() {
        if let Some(path) = selected {
            restore(&stt, path, settings.acceleration, &old_paths)?;
        }
        return Err(error);
    }
    if let Some(prepared) = prepared {
        stt.acknowledge_recovery_selection(prepared);
    }
    Ok(())
}

fn restore(
    stt: &stt::SttEngine,
    path: &str,
    acceleration: AccelerationMode,
    worker_paths: &stt::WorkerPaths,
) -> AppResult<()> {
    if stt
        .ensure_loaded(std::path::Path::new(path), acceleration, worker_paths)
        .is_err()
    {
        let message = "Пакет CUDA не активирован. Не удалось восстановить распознавание; выберите модель заново.";
        stt.reject_prepared_selection(message);
        return Err(AppError::Config(message.into()));
    }
    if let Some(previous) = stt.prepared_recovery_selection(&RecoverySelection {
        model_path: path.into(),
        acceleration,
        worker_paths: worker_paths.clone(),
    }) {
        stt.acknowledge_recovery_selection(previous);
    }
    Ok(())
}

fn ensure_idle(app: &AppHandle) -> AppResult<()> {
    let pipeline = app.state::<Pipeline>();
    let queue = app
        .state::<crate::application::service_control::ServiceControl>()
        .snapshot()
        .snapshot
        .queue;
    if pipeline.current_operation().is_some()
        || pipeline.has_active_service_operation()
        || super::dictation::workflow::is_busy(app)
        || super::wake_runtime::is_configuring(app)
        || queue.queued + queue.preparing + queue.transcribing > 0
        || super::models::has_active_downloads()
    {
        return Err(AppError::Busy(
            "Сначала завершите диктовку, проверку микрофона, загрузку моделей и задачи API.".into(),
        ));
    }
    Ok(())
}
