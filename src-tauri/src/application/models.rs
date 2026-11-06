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
    whisper_download::download_whisper_artifact(app, id.whisper_artifact()?).await
}

pub async fn set_stt_model(app: &AppHandle, _state: &AppState, id: ModelId) -> AppResult<()> {
    let activity = crate::application::updates::activity::lease()?;
    let transaction = crate::application::updates::settings_transaction(app).await;
    let owned_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let _transaction = transaction;
        let filename = if id.engine == SttEngine::Whisper {
            id.whisper_artifact()?.filename
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
        let artifact = id.whisper_artifact()?;
        let _registration =
            download::DownloadRegistration::begin(format!("whisper:{}", artifact.variant))?;
        let selected = app.state::<AppState>().settings().whisper_model_path;
        store::remove_model_file(
            &state::models_dir()?.join(artifact.filename),
            selected.as_deref(),
        )
    })
    .await
    .map_err(|_| AppError::Internal("model removal task failed".into()))?
}
/// Prepare the persisted source without blocking application startup: preload
/// a local model, or verify the remote client's metadata without sending audio.
/// Typed STT readiness keeps a remote startup error available to late UI reads.
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
    if settings.stt_source == crate::types::SttSource::Local
        && configured_stt_preload(&settings).is_none()
    {
        return;
    }
    let stt = app.state::<crate::pipeline::Pipeline>().stt().clone();

    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _activity = activity;
            // A queued startup preload may run after the source was changed.
            // Re-read the selection under the same transaction as settings
            // writes so it cannot reactivate a dormant local backend.
            let _transaction = tauri::async_runtime::block_on(
                crate::application::updates::settings_transaction(&app),
            );
            let settings = app.state::<AppState>().settings();
            prepare_startup(
                &settings,
                |model, acceleration| {
                    stt.ensure_model_loaded(
                        model,
                        acceleration,
                        &crate::stt::worker_paths_for_app(&app),
                        &fono_core::OperationCancellation::default(),
                    )
                },
                |settings| {
                    // Remote preparation performs metadata discovery only. It
                    // never loads dormant local weights or a server model.
                    let (attempt, result) = stt.ensure_configured_with_attempt(
                        settings,
                        &crate::stt::WorkerPaths::default(),
                        &fono_core::OperationCancellation::default(),
                    );
                    if let Err(error) = &result {
                        let _ = stt.report_remote_startup_failure(attempt, error, |readiness| {
                            crate::events::emit_stt_readiness(&app, readiness);
                        });
                    }
                    result
                },
            )
        })
        .await;
        match result {
            Ok(Ok(true)) => {
                tracing::info!("configured recognition source is ready after startup preparation")
            }
            Ok(Ok(false)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "background STT preload failed"),
            Err(error) => tracing::warn!(%error, "background STT preload task failed"),
        }
    });
}

fn prepare_startup(
    settings: &crate::types::Settings,
    local: impl FnOnce(&SttModelSelection, crate::types::AccelerationMode) -> AppResult<()>,
    remote: impl FnOnce(&crate::types::Settings) -> AppResult<()>,
) -> AppResult<bool> {
    match settings.stt_source {
        crate::types::SttSource::FonoServer => remote(settings).map(|()| true),
        crate::types::SttSource::Local => match configured_stt_preload(settings) {
            Some((model, acceleration)) => local(&model, acceleration).map(|()| true),
            None => Ok(false),
        },
    }
}

fn configured_stt_preload(
    settings: &crate::types::Settings,
) -> Option<(SttModelSelection, crate::types::AccelerationMode)> {
    // Remote selection retains the local preference for switching back, but
    // neither startup preload nor crash recovery may activate it implicitly.
    if settings.stt_source != crate::types::SttSource::Local {
        return None;
    }
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
    crate::types::validate_stt_selection(&selection)?;
    let model_path = std::path::PathBuf::from(&selection.local_path);
    if !model_path.exists() {
        return Err(AppError::ModelFileMissing);
    }

    let base = state.settings();
    let mut settings = base.clone();
    let same_selection = settings.selected_stt_model().as_ref() == Some(&selection);
    if settings.stt_source == crate::types::SttSource::Local
        && selection.id.engine != SttEngine::Whisper
    {
        // Explicit activation also selects the supported CPU route and automatic
        // language. Whisper-specific preferences remain available on return.
        settings.acceleration = crate::types::AccelerationMode::Auto;
        settings.language = "auto".into();
    }
    settings.stt_model = Some(selection);
    settings.reconcile_stt_model(Some(&base))?;

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
    stt.ensure_configured(
        &settings,
        &worker_paths,
        &fono_core::OperationCancellation::default(),
    )?;
    let prepared = if settings.stt_source == crate::types::SttSource::Local {
        stt.prepared_recovery_selection(&crate::stt::RecoverySelection {
            model_path,
            acceleration,
            worker_paths,
        })
    } else {
        None
    };

    if same_selection {
        rollback.commit();
        if let Some(prepared) = prepared {
            stt.acknowledge_recovery_selection(prepared);
        }
        return Ok(());
    }

    let settings = state.persist_settings_delta(&base, &settings)?;
    rollback.commit();
    if let Some(prepared) = prepared {
        stt.acknowledge_recovery_selection(prepared);
    }
    crate::events::emit_settings(app, &settings);
    tracing::info!(model = ?settings.whisper_model_path, "STT model selected and saved");
    Ok(())
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn remote_startup_checks_metadata_without_preparing_dormant_local_weights() {
        let settings = crate::types::Settings {
            stt_source: crate::types::SttSource::FonoServer,
            whisper_model_path: Some("PRIVATE_LOCAL/ggml-small.bin".into()),
            remote_stt: Some(crate::types::RemoteSttSettings {
                url: "https://PRIVATE_SERVER.invalid".into(),
                model: ModelId::whisper(WhisperModelSize::Tiny),
                display_name: None,
                ca_pem: None,
                fallback: Default::default(),
            }),
            ..Default::default()
        };
        let called = std::cell::Cell::new(false);
        assert!(prepare_startup(
            &settings,
            |_, _| panic!("remote startup must never load saved local weights"),
            |selected| {
                called.set(true);
                assert_eq!(selected.remote_stt, settings.remote_stt);
                Ok(())
            },
        )
        .unwrap());
        assert!(called.get());
    }

    #[test]
    fn local_startup_never_enters_credential_or_remote_discovery_work() {
        let configured = crate::types::Settings {
            whisper_model_path: Some("PRIVATE_LOCAL/ggml-small.bin".into()),
            acceleration: crate::types::AccelerationMode::Cpu,
            ..Default::default()
        };
        assert!(prepare_startup(
            &configured,
            |model, acceleration| {
                assert_eq!(model.id, ModelId::whisper(WhisperModelSize::Small));
                assert_eq!(acceleration, crate::types::AccelerationMode::Cpu);
                Ok(())
            },
            |_| panic!("a local source must not read remote credentials or contact a server"),
        )
        .unwrap());
        assert!(!prepare_startup(
            &crate::types::Settings::default(),
            |_, _| panic!("there is no local model to preload"),
            |_| panic!("missing local weights must not select a remote source"),
        )
        .unwrap());
    }

    #[test]
    fn latest_source_selection_and_remote_failure_do_not_trigger_local_fallback() {
        let initially_remote = crate::types::Settings {
            stt_source: crate::types::SttSource::FonoServer,
            ..Default::default()
        };
        let current = crate::types::Settings {
            stt_source: crate::types::SttSource::Local,
            ..initially_remote.clone()
        };
        // The blocking startup worker reads current settings under the settings
        // transaction instead of capturing a source when the task is queued.
        assert!(!prepare_startup(
            &current,
            |_, _| panic!("no model selected"),
            |_| panic!("an old remote source must not start discovery"),
        )
        .unwrap());
        let error = prepare_startup(
            &initially_remote,
            |_, _| panic!("a failed connection must not reactivate local weights"),
            |_| Err(AppError::Config("fixture incompatibility".into())),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Config(_)));
    }

    #[test]
    fn remote_source_skips_dormant_local_preload_and_restores_its_selection_on_return() {
        let local = crate::types::Settings {
            whisper_model_path: Some("/models/ggml-small.bin".into()),
            acceleration: crate::types::AccelerationMode::Cuda,
            ..Default::default()
        };
        let remote = crate::types::Settings {
            stt_source: crate::types::SttSource::FonoServer,
            ..local.clone()
        };
        assert!(configured_stt_preload(&remote).is_none());
        let restored = crate::types::Settings {
            stt_source: crate::types::SttSource::Local,
            ..remote
        };
        assert_eq!(
            configured_stt_preload(&restored),
            configured_stt_preload(&local)
        );
    }
}
