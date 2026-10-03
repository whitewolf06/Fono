//! Optional ordinary-mode postprocessing; failures retain the raw transcript.
use super::{lifecycle::Session, wait_for_cancellation};
use crate::{
    application::dictation_tail_diagnostics::DictationTailDiagnostic,
    llm::LlmClient,
    operation::TerminalReason,
    types::{AiMode, PipelineState, Transcript},
};

pub(super) async fn process(
    session: &Session,
    transcript: &Transcript,
    mut diagnostic: Option<&mut DictationTailDiagnostic>,
) -> Option<String> {
    if session.settings.ai_mode == AiMode::Off {
        if let Some(tail) = diagnostic.as_mut() {
            tail.postprocessor_skipped();
        }
        return Some(transcript.text.clone());
    }
    if !session.transition(PipelineState::Processing, TerminalReason::Completed) {
        if let Some(tail) = diagnostic.as_mut() {
            tail.postprocessor_cancelled();
        }
        return None;
    }
    let client = LlmClient::from_settings(&session.settings);
    let process = client.process(
        &transcript.text,
        session.settings.ai_mode,
        session.settings.clean_prompt.as_deref(),
    );
    let result = tokio::select! {
        result = process => Some(result),
        _ = wait_for_cancellation(session.cancellation.clone()) => None,
    };
    match result {
        None => {
            if let Some(tail) = diagnostic.as_mut() {
                tail.postprocessor_cancelled();
            }
            tracing::info!(
                operation = session.operation,
                "LLM request interrupted by dictation cancellation"
            );
            None
        }
        Some(Ok(text)) => {
            if let Some(tail) = diagnostic.as_mut() {
                tail.postprocessor_succeeded(text != transcript.text);
            }
            Some(text)
        }
        Some(Err(error)) => {
            if let Some(tail) = diagnostic.as_mut() {
                tail.postprocessor_fallback();
                session.error(crate::events::ErrorCodeV1::Llm, format!("LLM: {error}"));
            }
            tracing::warn!(%error, "LLM failed; returning raw transcript");
            Some(transcript.text.clone())
        }
    }
}
