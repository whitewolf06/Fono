//! IPC commands for confirming or discarding a recognized voice command.

use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_pending_voice_command(state: State<'_, AppState>) -> Option<String> {
    state.pending_voice_command()
}

#[tauri::command]
pub fn cancel_voice_command(state: State<'_, AppState>) {
    state.set_pending_voice_command(None);
}

#[tauri::command]
pub fn confirm_voice_command(app: AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    let text = state
        .take_pending_voice_command()
        .ok_or_else(|| AppError::Config("No pending voice command".into()))?;
    let settings = state.settings();
    crate::app_commands::execute(&text, &settings.launch_apps, settings.volume_step)
}
