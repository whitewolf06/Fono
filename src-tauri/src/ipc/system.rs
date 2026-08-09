use crate::error::AppResult;
use crate::state;
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
pub fn get_dictation_history() -> AppResult<Vec<DictationHistoryEntry>> {
    state::load_dictation_history()
}

#[tauri::command]
pub fn clear_dictation_history() -> AppResult<()> {
    state::clear_dictation_history()
}

#[tauri::command]
pub fn delete_dictation_history_entry(id: String) -> AppResult<()> {
    state::delete_dictation_history_entry(&id)
}
