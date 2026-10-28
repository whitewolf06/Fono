//! Desktop ownership and errors for shared atomic offline model packages.

use super::download::{emit_download, progress, DownloadRegistration};
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
    stt::offline_models::OfflineModelSpec,
    types::ModelId,
};
use std::path::Path;
use tauri::AppHandle;

pub(super) async fn download(app: AppHandle, id: ModelId) -> AppResult<()> {
    let spec = crate::stt::offline_models::specification(&id)?;
    let download_id = id.public_id();
    let registration = DownloadRegistration::begin(download_id.clone())?;
    let total = spec.files.iter().map(|file| file.bytes).sum();
    tauri::async_runtime::spawn_blocking(move || {
        let progress = progress(app.clone());
        emit_download(
            &app,
            &download_id,
            &spec.directory,
            ModelDownloadPhaseV1::Started,
            0,
            Some(total),
        );
        let result = match crate::state::models_dir() {
            Ok(directory) => fono_stt::models::offline_package::download(
                &directory,
                &spec,
                &registration.cancellation,
                &progress,
            )
            .map_err(AppError::from),
            Err(error) => {
                emit_download(
                    &app,
                    &download_id,
                    &spec.directory,
                    ModelDownloadPhaseV1::Failed,
                    0,
                    Some(total),
                );
                Err(error)
            }
        };
        if let Err(error) = &result {
            if !matches!(error, AppError::Cancelled(_)) {
                crate::events::emit_error(
                    &app,
                    crate::events::ErrorCodeV1::Download,
                    error.to_string(),
                    None,
                );
            }
        }
        result
    })
    .await
    .map_err(|_| AppError::Internal("model package download task failed".into()))?
}

pub(super) fn remove(
    target: &Path,
    spec: &OfflineModelSpec,
    selected: Option<&str>,
) -> AppResult<()> {
    fono_stt::models::offline_package::remove(target, spec, selected).map_err(AppError::from)
}

#[cfg(test)]
use std::{io::Read, sync::atomic::AtomicBool};

#[cfg(test)]
fn install(
    target: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    fetch: impl FnMut(
        &crate::stt::offline_models::OfflineArtifact,
    ) -> fono_stt::error::SttResult<Box<dyn Read>>,
    progress: impl FnMut(fono_stt::models::download::DownloadPhase, u64),
) -> AppResult<()> {
    fono_stt::models::offline_package::install(target, spec, cancellation, fetch, progress)
        .map_err(AppError::from)
}

#[cfg(test)]
fn is_installed(path: &Path, spec: &OfflineModelSpec) -> bool {
    fono_stt::models::offline_package::is_installed(path, spec)
}

#[cfg(test)]
#[path = "offline_package_integration_tests.rs"]
mod tests;
