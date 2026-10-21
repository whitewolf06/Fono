//! Optional ordinary-mode postprocessing; failures retain the raw transcript.
use super::{lifecycle::Session, wait_for_cancellation};
use crate::{
    application::dictation_tail_diagnostics::DictationTailDiagnostic,
    llm::LlmClient,
    types::{AiMode, LlmConnectionKind, LlmProvider, PipelineState, Settings, Transcript},
};
use fono_core::TerminalReason;

/// Only the operation snapshot changes; saved processing/translation choices survive.
/// Do this before deciding whether ordinary dictation needs a manual AI workflow.
pub(super) async fn prepare_ordinary(
    settings: &mut Settings,
) -> crate::error::AppResult<Option<LlmClient>> {
    if !super::workflow::should_process(settings)
        || (super::workflow::effective_preset(settings) == super::workflow::TextPreset::Raw
            && super::workflow::effective_language(settings).is_none())
    {
        return Ok(None);
    }
    let client = if settings.llm_profiles.is_empty() || settings.correction_profile().is_some() {
        Some(client_for_settings(settings).await?)
    } else {
        // A deleted/unselected profile must not fall back to another connection.
        None
    };
    Ok(apply_ordinary_readiness(settings, client))
}

fn apply_ordinary_readiness(
    settings: &mut Settings,
    client: Option<LlmClient>,
) -> Option<LlmClient> {
    let requires_key = settings.correction_profile().map_or(
        settings.llm_provider == LlmProvider::OpenAi,
        |profile| {
            profile.connection == LlmConnectionKind::Cloud
                || profile.provider == LlmProvider::OpenAi
        },
    );
    if client
        .as_ref()
        .is_some_and(|client| client.is_configured(requires_key))
    {
        return client;
    }
    settings.ai_mode = AiMode::Off;
    settings.processing_translation_enabled = false;
    tracing::debug!("Skipping ordinary postprocessing: connection setup is incomplete");
    None
}

/// Ordinary workflow processing reports errors instead of silently dropping a
/// requested translation. The caller keeps raw text for retry/explicit insertion.
pub(super) async fn selected(
    session: &Session,
    transcript: &Transcript,
    preset: super::workflow::TextPreset,
    language: Option<super::workflow::TranslationLanguage>,
    enabled: bool,
) -> crate::error::AppResult<Option<String>> {
    selected_prepared(session, transcript, preset, language, enabled, None).await
}

pub(super) async fn selected_prepared(
    session: &Session,
    transcript: &Transcript,
    preset: super::workflow::TextPreset,
    language: Option<super::workflow::TranslationLanguage>,
    enabled: bool,
    prepared: Option<LlmClient>,
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
        let client = match prepared {
            Some(client) => client,
            None => client_for_settings(&session.settings).await?,
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dictation::workflow::{self, ProcessingWorkflow, TranslationLanguage};

    #[test]
    fn incomplete_setup_skips_processing_translation_and_manual_wait_only_in_snapshot() {
        let saved = Settings {
            ai_mode: AiMode::Clean,
            processing_workflow: ProcessingWorkflow::Manual,
            processing_translation_enabled: true,
            processing_target_language: Some(TranslationLanguage::En),
            ..Settings::default()
        };
        for client in [
            None,
            Some(LlmClient::new("http://127.0.0.1:1234/v1", None, None)),
        ] {
            let mut snapshot = saved.clone();
            assert!(apply_ordinary_readiness(&mut snapshot, client).is_none());
            assert!(!workflow::should_process(&snapshot));
            assert!(!workflow::should_defer(&snapshot));
            assert_eq!(workflow::effective_language(&snapshot), None);
            assert_eq!(
                snapshot.processing_target_language,
                saved.processing_target_language
            );
        }
        assert!(workflow::should_process(&saved));
        assert!(workflow::should_defer(&saved));
        assert_eq!(
            workflow::effective_language(&saved),
            Some(TranslationLanguage::En)
        );
    }

    #[test]
    fn local_connection_without_key_is_ready_but_incomplete_cloud_is_skipped() {
        let mut settings = Settings {
            ai_mode: AiMode::Clean,
            ..Settings::default()
        };
        let client =
            || LlmClient::new("http://127.0.0.1:1234/v1", Some("local-model".into()), None);
        assert!(apply_ordinary_readiness(&mut settings, Some(client())).is_some());
        assert!(workflow::should_process(&settings));
        settings.llm_profiles[0].connection = LlmConnectionKind::Cloud;
        // Even a stale renderer flag must not replace an actual stored key.
        settings.llm_profiles[0].has_api_key = true;
        assert!(apply_ordinary_readiness(&mut settings, Some(client())).is_none());
        assert!(!workflow::should_process(&settings));
    }

    #[tokio::test]
    async fn deleted_profile_never_uses_a_configured_legacy_connection() {
        let mut settings = Settings {
            ai_mode: AiMode::Clean,
            llm_model: Some("legacy-model".into()),
            ..Settings::default()
        };
        settings.text_correction_llm.profile_id = Some("deleted".into());
        assert!(prepare_ordinary(&mut settings).await.unwrap().is_none());
        assert!(!workflow::should_process(&settings));
    }

    #[tokio::test]
    async fn raw_without_translation_needs_no_connection_or_credential_lookup() {
        let mut settings = Settings {
            ai_mode: AiMode::Clean,
            processing_preset: Some(workflow::TextPreset::Raw),
            ..Settings::default()
        };
        settings.text_correction_llm.profile_id = None;
        assert!(prepare_ordinary(&mut settings).await.unwrap().is_none());
        assert!(workflow::should_process(&settings));
        assert_eq!(
            workflow::effective_preset(&settings),
            workflow::TextPreset::Raw
        );
    }
}
