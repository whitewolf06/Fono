use crate::{
    application::selection_capture::{self, Runtime, Snapshot},
    error::{AppError, AppResult},
};
use tauri::{AppHandle, Manager, WebviewWindow};

// Memory snapshots only. Neither IPC nor ambient refresh initiates a field read.
#[tauri::command]
pub(crate) fn get_selection_capture(
    app: AppHandle,
    window: WebviewWindow,
) -> AppResult<Option<Snapshot>> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Выделение доступно только в главном окне Fono.".into(),
        ));
    }
    Ok(app.state::<Runtime>().snapshot())
}

#[tauri::command]
pub(crate) fn cancel_selection_capture(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
) -> AppResult<()> {
    if window.label() != "settings" {
        return Err(AppError::Config(
            "Выделение доступно только в главном окне Fono.".into(),
        ));
    }
    selection_capture::cancel(&app, operation_id);
    Ok(())
}
