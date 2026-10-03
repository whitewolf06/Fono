use super::download::{emit_download, DownloadRegistration};
use crate::{
    error::{AppError, AppResult},
    events::ModelDownloadPhaseV1,
    state::{self, AppState},
    types::WakeWordBackend,
};
use std::{path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Manager};
mod archive;
const RECEIPT: &str = ".fono-model-manifest.json";

pub fn is_kws_model_downloaded(app: &AppHandle) -> AppResult<bool> {
    let backend = app.state::<AppState>().settings().wake_backend;
    if backend == WakeWordBackend::WhisperExperimental {
        let settings = app.state::<AppState>().settings();
        return Ok(state::models_dir()?
            .join(settings.wake_word_model.filename())
            .is_file());
    }
    let Some(spec) = fono_wake::model_spec(backend) else {
        return Ok(false);
    };
    Ok(complete(
        &state::app_data_dir()?
            .join("kws-models")
            .join(spec.directory),
        spec,
        backend == WakeWordBackend::SherpaOnnx,
    ))
}
fn complete(dir: &Path, spec: &fono_wake::WakeModelSpec, legacy: bool) -> bool {
    let files_exist = spec
        .files
        .iter()
        .all(|file| std::fs::metadata(dir.join(file)).is_ok_and(|m| m.is_file() && m.len() > 0));
    if !files_exist {
        return false;
    }
    if legacy {
        return true;
    }
    let Ok(bytes) = std::fs::read(dir.join(RECEIPT)) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    receipt["archive_sha256"].as_str() == Some(spec.archive_sha256)
        && receipt["model"].as_str() == Some(spec.directory)
}
pub async fn download_kws_model(app: AppHandle) -> AppResult<()> {
    let backend = app.state::<AppState>().settings().wake_backend;
    let spec = fono_wake::model_spec(backend)
        .ok_or_else(|| {
            AppError::Config("Для выбранного движка нет WakeWord-модели для загрузки".into())
        })?
        .clone();
    let registration = DownloadRegistration::begin("kws".into())?;
    let base = state::app_data_dir()?.join("kws-models");
    tokio::fs::create_dir_all(&base).await?;
    let target = base.join(spec.directory);
    if target.exists() {
        return if complete(&target, &spec, backend == WakeWordBackend::SherpaOnnx) {
            Ok(())
        } else {
            Err(AppError::Config(format!(
                "Модель неполна: {}. Удалите повреждённую модель и повторите загрузку",
                target.display()
            )))
        };
    }
    let staging = base.join(format!(".{}.staging", spec.directory));
    if staging.exists() {
        tokio::fs::remove_dir_all(&staging).await?;
    }
    tokio::fs::create_dir_all(&staging).await?;
    emit_download(
        &app,
        "kws",
        spec.directory,
        ModelDownloadPhaseV1::Started,
        0,
        Some(spec.archive_bytes),
    );
    let result = async {
        let archive_path = staging.join("model.tar.bz2");
        archive::fetch_verified_archive(&app, &spec, &archive_path, &registration.cancellation)
            .await?;
        emit_download(
            &app,
            "kws",
            spec.directory,
            ModelDownloadPhaseV1::Extracting,
            spec.archive_bytes,
            Some(spec.archive_bytes),
        );
        let unpack_spec = spec.clone();
        let unpack_staging = staging.clone();
        let cancellation = registration.cancellation.clone();
        let activity = registration.blocking_lease();
        tauri::async_runtime::spawn_blocking(move || {
            let _activity = activity;
            archive::extract(&archive_path, &unpack_staging, &unpack_spec, &cancellation)
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;
        if registration.cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled(
                "WakeWord model download cancelled".into(),
            ));
        }
        let model = staging.join(spec.directory);
        let receipt = serde_json::json!({
            "model": spec.directory,
            "archive_sha256": spec.archive_sha256,
            "sample_rate": spec.sample_rate,
            "license": spec.license_url,
        });
        tokio::fs::write(
            model.join(RECEIPT),
            serde_json::to_vec_pretty(&receipt)
                .map_err(|error| AppError::Internal(error.to_string()))?,
        )
        .await?;
        if !complete(&model, &spec, false) {
            return Err(AppError::Config(
                "В архиве нет необходимых файлов WakeWord-модели".into(),
            ));
        }
        if target.exists() {
            return Err(AppError::Config(
                "Модель уже существует; текущие файлы сохранены".into(),
            ));
        }
        tokio::fs::rename(&model, &target).await?;
        emit_download(
            &app,
            "kws",
            spec.directory,
            ModelDownloadPhaseV1::Completed,
            spec.archive_bytes,
            Some(spec.archive_bytes),
        );
        Ok::<_, AppError>(())
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    if matches!(result, Err(AppError::Cancelled(_))) {
        emit_download(
            &app,
            "kws",
            spec.directory,
            ModelDownloadPhaseV1::Cancelled,
            0,
            None,
        );
    }
    if let Err(error) = &result {
        crate::events::emit_error(
            &app,
            crate::events::ErrorCodeV1::Download,
            error.to_string(),
            None,
        );
    }
    result
}
