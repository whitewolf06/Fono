//! Optional ordinary-mode postprocessing; failures retain the raw transcript.
use super::{lifecycle::Session, wait_for_cancellation};
use crate::{
    application::dictation_tail_diagnostics::DictationTailDiagnostic,
    llm::LlmClient,
    types::{AiMode, PipelineState, Transcript},
};
use fono_core::TerminalReason;

/// Ordinary workflow processing reports errors instead of silently dropping a
/// requested translation. The caller keeps raw text for retry/explicit insertion.
pub(super) async fn selected(
    session: &Session,
    transcript: &Transcript,
    preset: super::workflow::TextPreset,
    language: Option<super::workflow::TranslationLanguage>,
    enabled: bool,
) -> crate::error::AppResult<Option<String>> {
    if !enabled {
        return Ok(Some(transcript.text.clone()));
    }
    if session.settings.ai_mode == AiMode::Off {
        return Err(crate::error::AppError::Llm(
            "Включите обработку текста и выберите модель".into(),
        ));
    }
    if preset == super::workflow::TextPreset::Raw && language.is_none() {
        return Ok(Some(transcript.text.clone()));
    }
    if !session.transition(PipelineState::Processing, TerminalReason::Completed) {
        return Ok(None);
    }
    let client = LlmClient::from_settings(&session.settings);
    tokio::select! {
        result = client.process_preset(&transcript.text,preset,language,session.settings.processing_prompts.choice(preset)) => result.map(Some),
        _ = wait_for_cancellation(session.cancellation.clone()) => Ok(None),
    }
}

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
    let preset = super::workflow::effective_preset(&session.settings);
    let process = client.process_preset(
        &transcript.text,
        preset,
        super::workflow::effective_language(&session.settings),
        session.settings.processing_prompts.choice(preset),
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
