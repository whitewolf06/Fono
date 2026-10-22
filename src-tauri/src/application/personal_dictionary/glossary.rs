//! Bounded, immutable canonical terms for STT and LLM (F-03).
//! Construct from the operation's settings snapshot, never read mutable settings
//! during inference. This module neither sends terms nor changes the transcript.

use super::{normalize, validate};
use crate::types::Settings;
use std::collections::HashSet;

pub use fono_stt_protocol::prompt::WhisperGlossaryPrompt;
#[cfg(test)]
const MAX_WHISPER_PROMPT_TOKENS: usize = fono_stt_protocol::prompt::MAX_PROMPT_TOKENS;
use fono_stt_protocol::prompt::{MAX_BYTES, MAX_TERMS};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlossarySnapshot {
    terms: Vec<String>,
}

impl GlossarySnapshot {
    /// Older local-only consent never silently authorizes provider disclosure.
    pub fn for_llm(settings: &Settings) -> Self {
        if settings.personal_dictionary_llm_enabled {
            Self::from_settings(settings)
        } else {
            Self::default()
        }
    }
    pub fn from_settings(settings: &Settings) -> Self {
        if !settings.personal_dictionary_enabled
            || validate(&settings.personal_dictionary_entries).is_err()
        {
            return Self::default();
        }
        let mut seen = HashSet::new();
        let mut terms = Vec::new();
        let mut bytes = 0;
        let mut entries: Vec<_> = settings.personal_dictionary_entries.iter().collect();
        // Stable sort retains the user's order for equal/unknown priorities.
        // Frequency wins; equally frequent terms prefer the latest accepted use.
        entries.sort_by_key(|entry| {
            std::cmp::Reverse(settings.personal_dictionary_usage.priority(&entry.written))
        });
        for entry in entries {
            let term = entry.written.trim();
            if !seen.insert(normalize(term)) {
                continue;
            }
            let size = term.len() + if terms.is_empty() { 0 } else { 2 };
            if bytes + size > MAX_BYTES {
                continue;
            }
            bytes += size;
            terms.push(term.to_owned());
            if terms.len() == MAX_TERMS {
                break;
            }
        }
        Self { terms }
    }

    /// Only canonical spellings; spoken variants never become model instructions.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    /// Enforce the actual selected Whisper tokenizer's budget. Do not estimate
    /// tokens from UTF-8 length or cut a token stream midway through a term.
    /// CPU/CUDA use these owned tokens through set_tokens, avoiding the
    /// whisper-rs initial_prompt CString allocation path.
    pub fn whisper_prompt<E>(
        &self,
        tokenize: impl FnMut(&str) -> Result<Vec<i32>, E>,
    ) -> Result<WhisperGlossaryPrompt, E> {
        fono_stt_protocol::prompt::build(&self.terms, None, tokenize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PersonalDictionaryEntry;

    fn settings() -> Settings {
        Settings {
            personal_dictionary_enabled: true,
            personal_dictionary_entries: vec![
                PersonalDictionaryEntry {
                    written: "Kubernetes".into(),
                    spoken: vec!["кубер".into()],
                },
                PersonalDictionaryEntry {
                    written: "Fono".into(),
                    spoken: vec!["фоно".into()],
                },
            ],
            ..Settings::default()
        }
    }

    #[test]
    fn disabled_or_invalid_dictionary_produces_no_terms_or_tokenizer_calls() {
        let mut settings = settings();
        settings.personal_dictionary_enabled = false;
        let empty = GlossarySnapshot::from_settings(&settings);
        assert!(empty.terms().is_empty());
        let prompt = empty
            .whisper_prompt::<()>(|_| panic!("disabled glossary must stay private"))
            .unwrap();
        assert!(prompt.tokens.is_empty());
        assert_eq!(settings.personal_dictionary_entries.len(), 2);
        settings.personal_dictionary_enabled = true;
        settings.personal_dictionary_entries[0].written.push('\n');
        assert!(GlossarySnapshot::from_settings(&settings)
            .terms()
            .is_empty());
    }

    #[test]
    fn old_local_only_consent_does_not_authorize_llm_terms() {
        let mut settings = settings();
        assert!(GlossarySnapshot::for_llm(&settings).terms().is_empty());
        settings.personal_dictionary_llm_enabled = true;
        assert_eq!(
            GlossarySnapshot::for_llm(&settings).terms(),
            ["Kubernetes", "Fono"]
        );
        settings.personal_dictionary_enabled = false;
        assert!(GlossarySnapshot::for_llm(&settings).terms().is_empty());
        assert_eq!(settings.personal_dictionary_entries.len(), 2);
    }

    #[test]
    fn snapshot_is_frozen_and_contains_only_bounded_canonical_spellings() {
        let mut settings = settings();
        let snapshot = GlossarySnapshot::from_settings(&settings);
        settings.personal_dictionary_entries.clear();
        assert_eq!(snapshot.terms(), ["Kubernetes", "Fono"]);
        for index in 0..128 {
            settings
                .personal_dictionary_entries
                .push(PersonalDictionaryEntry {
                    written: format!("term{index:03}{}", "x".repeat(110)),
                    spoken: vec![format!("spoken{index:03}")],
                });
        }
        let bounded = GlossarySnapshot::from_settings(&settings);
        assert!(bounded.terms().len() <= MAX_TERMS);
        assert!(bounded.terms().join(", ").len() <= MAX_BYTES);
    }

    #[test]
    fn token_budget_uses_tokenizer_and_keeps_whole_terms() {
        let snapshot = GlossarySnapshot::from_settings(&settings());
        let prompt = snapshot
            .whisper_prompt::<()>(|text| Ok(vec![1; if text.contains(',') { 225 } else { 224 }]))
            .unwrap();
        assert_eq!(prompt.text, "Kubernetes");
        assert_eq!(prompt.tokens.len(), MAX_WHISPER_PROMPT_TOKENS);
        assert!(snapshot
            .whisper_prompt::<&str>(|_| Err("tokenization failed"))
            .is_err());
    }
}
