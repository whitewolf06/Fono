//! Owned, bounded glossary tokens. Terms are data, never model instructions.
pub const MAX_TERMS: usize = 32;
pub const MAX_BYTES: usize = 896;
pub const MAX_PROMPT_TOKENS: usize = 224;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WhisperGlossaryPrompt {
    pub text: String,
    pub tokens: Vec<i32>,
}

pub fn validate(terms: &[String]) -> Result<(), &'static str> {
    if terms.len() > MAX_TERMS
        || terms.iter().map(String::len).sum::<usize>() + terms.len().saturating_sub(1) * 2
            > MAX_BYTES
        || terms
            .iter()
            .any(|t| t.trim().is_empty() || t.contains(['\0', '\n', '\r']))
    {
        return Err("invalid or oversized glossary");
    }
    Ok(())
}

/// Preserve whole canonical terms; rolling context uses only remaining budget.
/// Empty terms/context never invoke the selected model's tokenizer.
pub fn build<E>(
    terms: &[String],
    context: Option<&str>,
    mut tokenize: impl FnMut(&str) -> Result<Vec<i32>, E>,
) -> Result<WhisperGlossaryPrompt, E> {
    let mut prompt = WhisperGlossaryPrompt::default();
    for term in terms {
        let candidate = if prompt.text.is_empty() {
            term.clone()
        } else {
            format!("{}, {term}", prompt.text)
        };
        let tokens = tokenize(&candidate)?;
        if tokens.len() <= MAX_PROMPT_TOKENS {
            prompt = WhisperGlossaryPrompt {
                text: candidate,
                tokens,
            };
        }
    }
    if let Some(context) = context.filter(|c| !c.is_empty()) {
        let candidate = if prompt.text.is_empty() {
            context.to_owned()
        } else {
            format!("{}. {context}", prompt.text)
        };
        let tokens = tokenize(&candidate)?;
        if tokens.len() <= MAX_PROMPT_TOKENS {
            prompt = WhisperGlossaryPrompt {
                text: candidate,
                tokens,
            };
        }
    }
    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_terms_have_priority_over_context_and_actual_tokens_obey_budget() {
        let terms = vec!["Kubernetes".into(), "WhiteLife".into()];
        let prompt = build::<()>(&terms, Some("rolling"), |s| {
            Ok(vec![1; if s.contains("rolling") { 225 } else { 224 }])
        })
        .unwrap();
        assert_eq!(prompt.text, "Kubernetes, WhiteLife");
        assert_eq!(prompt.tokens.len(), 224);
        assert_eq!(
            build::<()>(&[], None, |_| panic!("disabled prompt tokenized"))
                .unwrap()
                .tokens
                .len(),
            0
        );
    }
    #[test]
    fn invalid_private_terms_are_rejected_without_echoing_them() {
        for terms in [
            vec!["PRIVATE\nTERM".into()],
            vec!["x".repeat(897)],
            vec!["x".into(); 33],
        ] {
            assert_eq!(validate(&terms), Err("invalid or oversized glossary"));
        }
        assert!(validate(&[]).is_ok());
    }
}
