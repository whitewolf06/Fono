use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{DictationHistoryEntry, SpeechPeriodReport, SpeechSessionAnalysis};

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

#[tauri::command]
pub fn set_dictation_history_entry_analytics_included(
    app: AppHandle,
    state: State<'_, AppState>,
    queue: State<'_, crate::application::speech_analysis_queue::SpeechAnalysisQueue>,
    id: String,
    included: bool,
) -> AppResult<bool> {
    let found = crate::history::set_analytics_included(&id, included)?;
    if !found {
        return Ok(false);
    }

    let settings = state.settings();
    if included && settings.analytics_enabled && settings.history_enabled {
        queue.enqueue(id.clone());
    }
    let _ = app.emit("speech-analysis-changed", id);
    Ok(true)
}

#[tauri::command]
pub fn get_speech_session_analysis(id: String) -> AppResult<Option<SpeechSessionAnalysis>> {
    crate::history::session_analysis(&id)
}

#[tauri::command]
pub fn get_speech_period_report(from: String, to: String) -> AppResult<SpeechPeriodReport> {
    let from = parse_period_boundary(&from, "from")?;
    let to = parse_period_boundary(&to, "to")?;
    if from > to {
        return Err(AppError::Config(
            "Начало периода не может быть позже конца".into(),
        ));
    }
    crate::history::period_report(from, to)
}

fn parse_period_boundary(value: &str, field: &str) -> AppResult<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&chrono::Utc))
        .map_err(|_| AppError::Config(format!("Параметр {field} должен быть RFC 3339 датой")))
}

#[cfg(test)]
mod tests {
    use super::parse_period_boundary;

    #[test]
    fn period_boundaries_accept_rfc3339_utc_dates() {
        let value = parse_period_boundary("2026-08-31T12:00:00Z", "from").unwrap();
        assert_eq!(value.to_rfc3339(), "2026-08-31T12:00:00+00:00");
    }

    #[test]
    fn period_boundaries_reject_non_rfc3339_dates() {
        assert!(parse_period_boundary("31.08.2026", "from").is_err());
    }
}
