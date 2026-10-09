//! IPC commands for confirming or discarding a recognized voice command.

use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;
use crate::state::AppState;

/// Typed command-preview contract.
#[tauri::command]
pub fn get_pending_command_proposal(
    state: State<'_, AppState>,
) -> Option<crate::types::CommandProposal> {
    state.pending_command_proposal()
}

#[tauri::command]
pub fn cancel_voice_command(state: State<'_, AppState>) {
    state.set_pending_command_proposal(None);
}

#[tauri::command]
pub fn confirm_voice_command(app: AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    let proposal = state.take_pending_command_proposal()?;
    crate::app_commands::execute(
        &proposal.normalized_action,
        &proposal.settings_snapshot.launch_apps,
        proposal.settings_snapshot.volume_step,
    )
}
