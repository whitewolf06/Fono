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
    pub fn start(app: AppHandle) -> Self {
        let (sender, receiver) = unbounded::<String>();
        let event_app = app.clone();
        std::thread::Builder::new()
            .name("fono-speech-analysis".into())
            .spawn(move || {
                for session_id in receiver {
                    process_session(&event_app, &session_id);
                }
            })
            .expect("speech analysis worker thread should start");

        let queue = Self { sender };
        match crate::history::pending_analysis_ids() {
            Ok(ids) => {
                for id in ids {
                    queue.enqueue(id);
                }
            }
            Err(error) => tracing::warn!(%error, "could not restore pending speech analyses"),
        }
        queue
    }

    pub fn enqueue(&self, session_id: String) {
        if let Err(error) = self.sender.send(session_id) {
            tracing::warn!(%error, "speech analysis queue is unavailable");
        }
    }
}

fn process_session(app: &AppHandle, session_id: &str) {
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
            generate_llm_recommendation(app, session_id, &original_text);
            let _ = app.emit("speech-analysis-changed", session_id.to_owned());
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(%error, session_id, "could not save speech analysis");
            let _ = crate::history::mark_analysis_failed(session_id, "storage_unavailable");
        }
    }
}

fn generate_llm_recommendation(app: &AppHandle, session_id: &str, original_text: &str) {
    let settings = app.state::<crate::state::AppState>().settings();
    let assignment = &settings.speech_analysis_llm;
    if !settings.analytics_enabled || !settings.speech_trainer_enabled || !assignment.enabled {
        return;
    }
    let Some(profile) = settings.speech_analysis_profile() else {
        let _ = crate::history::mark_recommendation_failed(session_id, "profile_not_configured");
        return;
    };
    if profile.connection == LlmConnectionKind::Cloud
        && assignment.data_scope != SpeechLlmDataScope::MetricsOnly
        && !assignment.cloud_consent
    {
        let _ = crate::history::mark_recommendation_failed(session_id, "cloud_consent_required");
        return;
    }
    let Some(analysis) = crate::history::session_analysis(session_id).ok().flatten() else {
        return;
    };
    let findings = match assignment.data_scope {
        SpeechLlmDataScope::MetricsOnly => None,
        SpeechLlmDataScope::Findings | SpeechLlmDataScope::OriginalText => {
            Some(analysis.findings.as_slice())
        }
    };
    let text = (assignment.data_scope == SpeechLlmDataScope::OriginalText).then_some(original_text);
    let client = crate::llm::LlmClient::from_profile(profile, assignment.model.as_deref());
    let result = tauri::async_runtime::block_on(client.analyze_speech(&analysis, findings, text));
    match result {
        Ok(recommendation) => {
            if let Err(error) = crate::history::complete_recommendation(session_id, recommendation)
            {
                tracing::warn!(%error, session_id, "could not save LLM speech recommendation");
            }
        }
        Err(error) => {
            tracing::warn!(%error, session_id, "LLM speech recommendation failed");
            let _ = crate::history::mark_recommendation_failed(session_id, "llm_request_failed");
        }
    }
}
