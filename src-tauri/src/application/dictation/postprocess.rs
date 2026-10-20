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
    if let Some(snippet) =
        crate::application::snippets::matching(&session.settings, &transcript.text)
    {
        let result = crate::application::snippets::expand_native(
            snippet.clone(),
            session.cancellation.clone(),
        )
        .await;
        if !session.active("snippet expansion returned") || session.cancellation.is_cancelled() {
            return Ok(None);
        }
        return result.map(Some);
    }
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
    let process = async {
        let client = client_for_settings(&session.settings).await?;
        client
            .process_preset_with_glossary(
                &transcript.text,
                preset,
                language,
                session.settings.processing_prompts.choice(preset),
                crate::application::personal_dictionary::GlossarySnapshot::for_llm(
                    &session.settings,
                )
                .terms(),
            )
            .await
    };
    tokio::select! {
        biased;
        _ = wait_for_cancellation(session.cancellation.clone()) => Ok(None),
        result = process => result.map(Some),
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
    let preset = super::workflow::effective_preset(&session.settings);
    let process = async {
        let client = client_for_settings(&session.settings).await?;
        client
            .process_preset_with_glossary(
                &transcript.text,
                preset,
                super::workflow::effective_language(&session.settings),
                session.settings.processing_prompts.choice(preset),
                crate::application::personal_dictionary::GlossarySnapshot::for_llm(
                    &session.settings,
                )
                .terms(),
            )
            .await
    };
    let result = tokio::select! {
        biased;
        _ = wait_for_cancellation(session.cancellation.clone()) => None,
        result = process => Some(result),
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

async fn client_for_settings(
    settings: &crate::types::Settings,
) -> crate::error::AppResult<LlmClient> {
    let settings = settings.clone();
    let lease = crate::application::updates::activity::lease()?;
    tauri::async_runtime::spawn_blocking(move || {
        // Reading a native credential can outlive cancellation of the request.
        let _lease = lease;
        LlmClient::from_settings(&settings)
    })
    .await
    .map_err(|_| crate::error::AppError::Internal("LLM client preparation task failed".into()))
}
