use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;
use crate::types::DictationHistoryEntry;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    version: &'static str,
    revision: &'static str,
    profile: &'static str,
}

#[tauri::command]
pub fn get_build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        revision: env!("FONO_BUILD_REVISION"),
        profile: env!("FONO_BUILD_PROFILE"),
    }
}

#[tauri::command]
pub fn get_dictation_history(state: State<'_, AppState>) -> AppResult<Vec<DictationHistoryEntry>> {
    let settings = state.settings();
    crate::history::list(
        settings.analytics_enabled && settings.history_enabled,
        settings.analytics_retention_days,
    )
}

#[tauri::command]
pub fn clear_dictation_history() -> AppResult<()> {
    crate::history::clear()
}

#[tauri::command]
pub fn delete_dictation_history_entry(id: String) -> AppResult<()> {
    crate::history::delete(&id)
}
