//! Single local worker for saved, opt-in speech-analysis sessions.

use crossbeam_channel::{unbounded, Sender};
use tauri::{AppHandle, Emitter, Manager};

use crate::types::{LlmConnectionKind, SpeechLlmDataScope};

/// A serial queue prevents concurrent updates of the same session. Requeued
/// session IDs are harmless: history only accepts a result while it is pending.
pub struct SpeechAnalysisQueue {
    sender: Sender<String>,
}

impl SpeechAnalysisQueue {
    pub fn start(app: AppHandle) -> std::io::Result<Self> {
        let (sender, receiver) = unbounded::<String>();
        let event_app = app.clone();
        std::thread::Builder::new()
            .name("fono-speech-analysis".into())
            .spawn(move || {
                for session_id in receiver {
                    process_session(&event_app, &session_id);
                }
            })?;

        let queue = Self { sender };
        match crate::history::pending_analysis_ids() {
            Ok(ids) => {
                for id in ids {
                    queue.enqueue(id);
                }
            }
            Err(error) => tracing::warn!(%error, "could not restore pending speech analyses"),
        }
        Ok(queue)
    }

    pub fn enqueue(&self, session_id: String) {
        if let Err(error) = self.sender.send(session_id) {
            tracing::warn!(%error, "speech analysis queue is unavailable");
        }
    }
}

fn process_session(app: &AppHandle, session_id: &str) {
    let Ok(_lease) = crate::application::updates::activity::lease() else {
        // Pending sessions are restored at the next start after installation.
        return;
    };
    let original_text = match crate::history::pending_analysis_input(session_id) {
        Ok(Some(text)) => text,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, session_id, "could not load pending speech analysis");
            return;
        }
    };
    let analysis = crate::speech_metrics::analyze_russian_speech(&original_text);

    match crate::history::complete_analysis(session_id, analysis) {
        Ok(true) => {
            generate_llm_recommendation(app, session_id);
            let _ = app.emit("speech-analysis-changed", session_id.to_owned());
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(%error, session_id, "could not save speech analysis");
            let _ = crate::history::mark_analysis_failed(session_id, "storage_unavailable");
        }
    }
}

fn generate_llm_recommendation(app: &AppHandle, session_id: &str) {
    let state = app.state::<crate::state::AppState>();
    let (settings, revision) = {
        let settings = state.settings.lock();
        (settings.clone(), state.settings_revision())
    };
    let assignment = &settings.speech_analysis_llm;
    if !settings.history_enabled
        || !settings.analytics_enabled
        || !settings.speech_trainer_enabled
        || !assignment.enabled
    {
        return;
    }
    let Ok(request) = crate::application::trainer_ai::SessionRequest::begin(session_id) else {
        return;
    };
    let Ok(Some((original_text, analysis, source_fingerprint))) =
        crate::history::recommendation_input(session_id, &settings)
    else {
        return;
    };
    let Some(profile) = settings.speech_analysis_profile() else {
        fail_current_recommendation(
            &state,
            &request,
            revision,
            session_id,
            &source_fingerprint,
            "profile_not_configured",
        );
        return;
    };
    if profile.connection == LlmConnectionKind::Cloud && !assignment.cloud_consent {
        fail_current_recommendation(
            &state,
            &request,
            revision,
            session_id,
            &source_fingerprint,
            "cloud_consent_required",
        );
        return;
    }
    let findings = match assignment.data_scope {
        SpeechLlmDataScope::MetricsOnly => None,
        SpeechLlmDataScope::Findings | SpeechLlmDataScope::OriginalText => {
            Some(analysis.findings.as_slice())
        }
    };
    let text = (assignment.data_scope == SpeechLlmDataScope::OriginalText)
        .then_some(original_text.as_str());
    let client = crate::llm::LlmClient::from_profile(profile, assignment.model.as_deref());
    let result = tauri::async_runtime::block_on(async {
        if state.settings_revision() != revision {
            return Err(crate::error::AppError::Cancelled(
                "Настройки изменились до начала анализа".into(),
            ));
        }
        tokio::select! {
            biased;
            _ = request.token.cancelled() => Err(crate::error::AppError::Cancelled("Диктовка исключена или настройки изменились".into())),
            result = client.analyze_speech(&analysis, findings, text) => result,
        }
    });
    let current_settings = state.settings.lock();
    if request.token.is_cancelled() || state.settings_revision() != revision {
        return;
    }
    match result {
        Ok(recommendation) => {
            if let Err(error) = crate::history::complete_recommendation_if_current(
                session_id,
                &source_fingerprint,
                recommendation,
                &current_settings,
            ) {
                tracing::warn!(%error, session_id, "could not save LLM speech recommendation");
            }
        }
        Err(error) => {
            tracing::warn!(%error, session_id, "LLM speech recommendation failed");
            let _ = crate::history::mark_recommendation_failed_if_current(
                session_id,
                &source_fingerprint,
                "llm_request_failed",
                &current_settings,
            );
        }
    }
}

fn fail_current_recommendation(
    state: &crate::state::AppState,
    request: &crate::application::trainer_ai::SessionRequest,
    revision: u64,
    session_id: &str,
    source_fingerprint: &str,
    reason: &str,
) {
    let current = state.settings.lock();
    if !request.token.is_cancelled() && state.settings_revision() == revision {
        let _ = crate::history::mark_recommendation_failed_if_current(
            session_id,
            source_fingerprint,
            reason,
            &current,
        );
    }
}
