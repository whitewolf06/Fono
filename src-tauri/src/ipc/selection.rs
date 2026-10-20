use crate::{
    application::selection_capture::{self, Runtime, Snapshot},
    error::{AppError, AppResult},
};
use tauri::{AppHandle, Manager, WebviewWindow};

fn require_main(window: &WebviewWindow) -> AppResult<()> {
    if !matches!(window.label(), "settings" | "overlay") {
        return Err(AppError::Config(
            "Выделение доступно только в главном окне Fono.".into(),
        ));
    }
    Ok(())
}

// Memory snapshots only. Neither IPC nor ambient refresh initiates a field read.
#[tauri::command]
pub(crate) fn get_selection_capture(
    app: AppHandle,
    window: WebviewWindow,
) -> AppResult<Option<Snapshot>> {
    require_main(&window)?;
    Ok(app.state::<Runtime>().snapshot())
}

#[tauri::command]
pub(crate) fn cancel_selection_capture(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::cancel(&app, operation_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn start_selection_instruction(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::edit::start_voice(app, operation_id)
}

#[tauri::command]
pub(crate) fn stop_selection_instruction(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    edit_operation_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::edit::stop_voice(&app, operation_id, edit_operation_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn cancel_selection_edit(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    edit_operation_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::edit::cancel(&app, operation_id, edit_operation_id);
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SelectionEditInput {
    instruction: String,
    transfer_confirmed: bool,
}

#[tauri::command]
pub(crate) async fn transform_selection(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    input: SelectionEditInput,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::edit::transform(
        app,
        operation_id,
        input.instruction,
        input.transfer_confirmed,
    )
}

#[tauri::command]
pub(crate) fn arm_selection_replace(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    result_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::delivery::arm(&app, operation_id, result_id)
}

#[tauri::command]
pub(crate) fn cancel_selection_delivery(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    result_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::delivery::disarm(&app, operation_id, result_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn copy_selection_result(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    result_id: u64,
) -> AppResult<()> {
    require_main(&window)?;
    selection_capture::delivery::copy(app, operation_id, result_id)
}

#[tauri::command]
pub(crate) async fn insert_selection_result(
    app: AppHandle,
    window: WebviewWindow,
    operation_id: u64,
    result_id: u64,
) -> AppResult<()> {
    // Only the non-activating overlay offers direct insertion. The main editor
    // must arm and return to the source field through the separate shortcut.
    if window.label() != "overlay" {
        return Err(AppError::Config(
            "Прямая замена доступна только в индикаторе Fono.".into(),
        ));
    }
    selection_capture::delivery::insert(app, operation_id, result_id)
}
