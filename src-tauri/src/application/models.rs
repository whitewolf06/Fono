//! Speech model readiness and selection; downloads live in bounded adapters.
mod catalog;
mod download;
#[cfg(feature = "offline-stt")]
mod offline_package;
mod selection_rollback;
mod store;
pub use catalog::{list_stt_models, SttModelInfo};
pub(crate) use selection_rollback::SelectionRollback;
#[cfg(feature = "wake-word")]
mod wake_download;
mod whisper_download;
use crate::{
    error::{AppError, AppResult},
    state::{self, AppState},
    types::{ModelId, SttEngine, SttModelSelection, WhisperModelInfo, WhisperModelSize},
};
pub use download::{cancel_download, has_active_downloads};
use tauri::{AppHandle, Manager};
#[cfg(feature = "wake-word")]
pub use wake_download::{download_kws_model, is_kws_model_downloaded};
pub use whisper_download::download_whisper_model;

pub async fn download_stt_model(app: AppHandle, id: ModelId) -> AppResult<()> {
    if id.engine != SttEngine::Whisper {
        #[cfg(feature = "offline-stt")]
        return offline_package::download(app, id).await;
        #[cfg(not(feature = "offline-stt"))]
        return Err(AppError::Config(
            "Offline STT is unavailable in this build".into(),
        ));
    }
    download_whisper_model(app, id.whisper_size()?.api_identifier().into()).await
}

pub async fn set_stt_model(app: &AppHandle, _state: &AppState, id: ModelId) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let transaction = crate::application::updates::settings_transaction(app).await;
    let owned_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _transaction = transaction;
        let filename = if id.engine == SttEngine::Whisper {
            id.whisper_size()?.filename().to_owned()
        } else {
            #[cfg(feature = "offline-stt")]
            {
                crate::stt::offline_models::specification(&id)?.directory
            }
            #[cfg(not(feature = "offline-stt"))]
            return Err(AppError::Config(
                "Offline STT is unavailable in this build".into(),
            ));
        };
        let selection = SttModelSelection {
            id,
            local_path: state::models_dir()?
                .join(filename)
                .to_string_lossy()
                .into_owned(),
        };
        set_model_blocking(&owned_app, owned_app.state::<AppState>().inner(), selection)
    })
    .await
    .map_err(|_| AppError::Internal("model selection task failed".into()))?
}

pub async fn remove_stt_model(app: AppHandle, id: ModelId) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let transaction = crate::application::updates::settings_transaction(&app).await;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _transaction = transaction;
        if id.engine != SttEngine::Whisper {
            #[cfg(feature = "offline-stt")]
            {
                let spec = crate::stt::offline_models::specification(&id)?;
                let _registration = download::DownloadRegistration::begin(id.public_id())?;
                let selected = app.state::<AppState>().settings().selected_stt_model();
                return offline_package::remove(
                    &state::models_dir()?.join(&spec.directory),
                    &spec,
                    selected.as_ref().map(|m| m.local_path.as_str()),
                );
            }
            #[cfg(not(feature = "offline-stt"))]
            return Err(AppError::Config(
                "Offline STT is unavailable in this build".into(),
            ));
        }
        let size = id.whisper_size()?;
        let _registration =
            download::DownloadRegistration::begin(format!("whisper:{}", size.api_identifier()))?;
        let selected = app.state::<AppState>().settings().whisper_model_path;
        store::remove_model_file(
            &state::models_dir()?.join(size.filename()),
            selected.as_deref(),
        )
    })
    .await
    .map_err(|_| AppError::Internal("model removal task failed".into()))?
}
/// Starts a best-effort background preload of the model selected in persisted
/// settings.  It deliberately does not make application startup fail: the
/// typed STT readiness IPC remains the source of truth for a missing model or
/// unavailable worker, and dictation still uses the same load gate.
pub fn preload_configured_stt(app: AppHandle) {
    let recovery_app = app.clone();
    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    if let Err(error) = stt.enable_recovery(std::sync::Arc::new(move || {
        let settings = recovery_app.state::<AppState>().settings();
        let (model, acceleration) = configured_stt_preload(&settings)?;
        if model.id.engine != SttEngine::Whisper {
            return None;
        }
        Some(crate::stt::RecoverySelection {
            model_path: model.local_path.into(),
            acceleration,
            // An optional package may have been installed since startup.
            worker_paths: crate::stt::worker_paths_for_app(&recovery_app),
        })
    })) {
        tracing::warn!(%error, "STT recovery could not be enabled");
    }
    let Ok(activity) = crate::application::updates::activity::lease() else {
        return;
    };
    let settings = app.state::<AppState>().settings();
    let Some((model, acceleration)) = configured_stt_preload(&settings) else {
        return;
    };
    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    let worker_paths = crate::stt::worker_paths_for_app(&app);

    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _activity = activity;
            stt.ensure_model_loaded(
                &model,
                acceleration,
                &worker_paths,
                &fono_core::OperationCancellation::default(),
            )
        })
        .await;
        match result {
            Ok(Ok(())) => tracing::info!("configured STT model is ready after background preload"),
            Ok(Err(error)) => tracing::warn!(%error, "background STT preload failed"),
            Err(error) => tracing::warn!(%error, "background STT preload task failed"),
        }
    });
}

fn configured_stt_preload(
    settings: &crate::types::Settings,
) -> Option<(SttModelSelection, crate::types::AccelerationMode)> {
    settings
        .selected_stt_model()
        .map(|model| (model, settings.acceleration))
}

pub fn list_whisper_models() -> AppResult<Vec<WhisperModelInfo>> {
    let dir = state::models_dir()?;
    let mut out = Vec::new();
    for size in WhisperModelSize::ALL {
        let filename = size.filename();
        let path = dir.join(filename);
        let (local_path, bytes) = if path.exists() {
            let meta = std::fs::metadata(&path).ok();
            (
                Some(path.to_string_lossy().to_string()),
                meta.map(|item| item.len()),
            )
        } else {
            (None, Some(size.exact_bytes()))
        };
        out.push(WhisperModelInfo {
            filename: filename.to_string(),
            size,
            local_path,
            bytes,
        });
    }
    Ok(out)
}

/// Makes a downloaded model active only after the STT runtime has accepted it.
/// This keeps persisted settings, the running engine and renderer events in
/// sync, so a successful "Choose" action never leaves a draft-only selection.
pub async fn set_whisper_model(app: &AppHandle, _state: &AppState, path: String) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let settings_transaction = crate::application::updates::settings_transaction(app).await;
    let owned_app = app.clone();
    // All native and disk phases share one owned transaction. Dropping the IPC
    // waiter cannot release admission or abandon settings after loading a model.
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _settings_transaction = settings_transaction;
        let state = owned_app.state::<AppState>();
        set_model_blocking(
            &owned_app,
            state.inner(),
            SttModelSelection::from_whisper_path(path),
        )
    })
    .await
    .map_err(|_| AppError::Internal("model selection task failed".into()))?
}

fn set_model_blocking(
    app: &AppHandle,
    state: &AppState,
    selection: SttModelSelection,
) -> AppResult<()> {
    selection.validate()?;
    let model_path = std::path::PathBuf::from(&selection.local_path);
    if !model_path.exists() {
        return Err(AppError::ModelFileMissing);
    }

    let base = state.settings();
    let mut settings = base.clone();
    let same_selection = settings.selected_stt_model().as_ref() == Some(&selection);
    if selection.id.engine != SttEngine::Whisper {
        // Explicit activation also selects the supported CPU route and automatic
        // language. Whisper-specific preferences remain available on return.
        settings.acceleration = crate::types::AccelerationMode::Auto;
        settings.language = "auto".into();
    }

    let _configuration = super::capture_configuration::reserve_configuration(|| {
        if app
            .state::<crate::pipeline::Pipeline>()
            .current_operation()
            .is_some()
            || app
                .state::<crate::pipeline::Pipeline>()
                .has_active_service_operation()
            || super::dictation::workflow::is_busy(app)
        {
            return Err(AppError::Busy(
                "Сначала завершите текущую диктовку перед выбором модели".into(),
            ));
        }
        Ok(())
    })?;

    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();
    let acceleration = settings.acceleration;
    let worker_paths = crate::stt::worker_paths_for_app(app);
    let mut rollback = SelectionRollback::new(stt.clone(), base.clone(), worker_paths.clone());
    stt.reset_recovery_budget();
    stt.ensure_model_loaded(
        &selection,
        acceleration,
        &worker_paths,
        &fono_core::OperationCancellation::default(),
    )?;
    let prepared = stt.prepared_recovery_selection(&crate::stt::RecoverySelection {
        model_path,
        acceleration,
        worker_paths,
    });

    if same_selection {
        rollback.commit();
        if let Some(prepared) = prepared {
            stt.acknowledge_recovery_selection(prepared);
        }
        return Ok(());
    }

    settings.stt_model = Some(selection);
    settings.reconcile_stt_model(Some(&base))?;
    let settings = state.persist_settings_delta(&base, &settings)?;
    rollback.commit();
    if let Some(prepared) = prepared {
        stt.acknowledge_recovery_selection(prepared);
    }
    crate::events::emit_settings(app, &settings);
    tracing::info!(model = ?settings.whisper_model_path, "STT model selected and saved");
    Ok(())
}
