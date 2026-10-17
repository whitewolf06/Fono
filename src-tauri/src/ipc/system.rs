use tauri::{AppHandle, Emitter, Manager, State};

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
pub async fn get_dictation_history(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DictationHistoryEntry>> {
    let settings = state.settings();
    super::blocking::run(move || {
        crate::history::for_app(&app).list(
            settings.analytics_enabled && settings.history_enabled,
            settings.analytics_retention_days,
        )
    })
    .await
}

#[tauri::command]
pub async fn clear_dictation_history(app: AppHandle) -> AppResult<()> {
    super::blocking::run(move || crate::history::for_app(&app).clear()).await
}

#[tauri::command]
pub async fn delete_dictation_history_entry(app: AppHandle, id: String) -> AppResult<()> {
    super::blocking::run(move || crate::history::for_app(&app).delete(&id)).await
}

#[tauri::command]
pub async fn set_dictation_history_entry_analytics_included(
    app: AppHandle,
    id: String,
    included: bool,
) -> AppResult<bool> {
    super::blocking::run(move || {
        let found = crate::history::for_app(&app).set_analytics_included(&id, included)?;
        if !found {
            return Ok(false);
        }

        let settings = app.state::<AppState>().settings();
        if included
            && settings.analytics_enabled
            && settings.speech_trainer_enabled
            && settings.history_enabled
        {
            app.state::<crate::application::speech_analysis_queue::SpeechAnalysisQueue>()
                .enqueue(id.clone());
        }
        let _ = app.emit("speech-analysis-changed", id);
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn get_speech_session_analysis(
    app: AppHandle,
    id: String,
) -> AppResult<Option<SpeechSessionAnalysis>> {
    super::blocking::run(move || crate::history::for_app(&app).session_analysis(&id)).await
}

#[tauri::command]
pub async fn get_speech_period_report(
    app: AppHandle,
    from: String,
    to: String,
) -> AppResult<SpeechPeriodReport> {
    let from = parse_period_boundary(&from, "from")?;
    let to = parse_period_boundary(&to, "to")?;
    if from > to {
        return Err(AppError::Config(
            "Начало периода не может быть позже конца".into(),
        ));
    }
    super::blocking::run(move || crate::history::for_app(&app).period_report(from, to)).await
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
