use crate::{
    application::cuda_packages::{CudaPackageService, CudaPackageSnapshot},
    error::{AppError, AppResult},
};
use tauri::{AppHandle, Manager, WebviewWindow};

fn require_main(window: &WebviewWindow) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Установить пакет CUDA можно только в основном окне Fono.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn get_cuda_package_status(app: AppHandle) -> CudaPackageSnapshot {
    app.state::<CudaPackageService>().snapshot()
}

#[tauri::command]
pub async fn install_cuda_package(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: String,
) -> AppResult<CudaPackageSnapshot> {
    require_main(&window)?;
    // Capture single-flight + admission synchronously, BEFORE queueing work.
    let operation = app.state::<CudaPackageService>().begin(operation_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<CudaPackageService>().install_blocking(
            operation,
            crate::stt::probe_cuda_worker,
            |package, commit| crate::application::cuda_activation::activate(&app, package, commit),
        )
    })
    .await
    .map_err(|_| {
        AppError::Config("Не удалось завершить подготовку CUDA. Повторите установку.".into())
    })?
}

#[tauri::command]
pub fn cancel_cuda_package(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: String,
) -> AppResult<CudaPackageSnapshot> {
    require_main(&window)?;
    Ok(app.state::<CudaPackageService>().cancel(&operation_id))
}
