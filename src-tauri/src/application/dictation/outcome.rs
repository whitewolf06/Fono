//! A dictation result is a domain decision, never an empty-text sentinel.
use crate::{pipeline::Pipeline, types::Transcript};
use fono_core::TerminalReason;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "transcript", rename_all = "snake_case")]
pub enum DictationOutcome {
    Completed(Transcript),
    Deferred(Transcript),
    NoSpeech,
    Cancelled,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationOutcomeKind {
    Completed,
    Deferred,
    NoSpeech,
    Cancelled,
    Superseded,
}

impl DictationOutcome {
    pub(crate) fn kind(&self) -> DictationOutcomeKind {
        match self {
            Self::Completed(_) => DictationOutcomeKind::Completed,
            Self::Deferred(_) => DictationOutcomeKind::Deferred,
            Self::NoSpeech => DictationOutcomeKind::NoSpeech,
            Self::Cancelled => DictationOutcomeKind::Cancelled,
            Self::Superseded => DictationOutcomeKind::Superseded,
        }
    }

    pub(crate) fn terminal_reason(&self) -> TerminalReason {
        match self {
            Self::Cancelled | Self::Superseded => TerminalReason::Cancelled,
            Self::Completed(_) | Self::Deferred(_) | Self::NoSpeech => TerminalReason::Completed,
        }
    }

    pub(crate) fn interrupted(pipeline: &Pipeline, operation: u64) -> Self {
        if pipeline.is_operation_superseded(operation) {
            Self::Superseded
        } else {
            Self::Cancelled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(text: &str) -> Transcript {
        Transcript {
            text: text.into(),
            detected_language: None,
            transcribe_secs: None,
            audio_secs: None,
            device: None,
        }
    }

    #[test]
    fn serialization_distinguishes_every_outcome_without_text_heuristics() {
        for (outcome, kind) in [
            (DictationOutcome::Completed(transcript("")), "completed"),
            (
                DictationOutcome::Deferred(transcript("сохранено")),
                "deferred",
            ),
            (DictationOutcome::NoSpeech, "no_speech"),
            (DictationOutcome::Cancelled, "cancelled"),
            (DictationOutcome::Superseded, "superseded"),
        ] {
            let value = serde_json::to_value(&outcome).unwrap();
            assert_eq!(value["kind"], kind);
            assert_eq!(
                value.get("transcript").is_some(),
                matches!(
                    outcome,
                    DictationOutcome::Completed(_) | DictationOutcome::Deferred(_)
                )
            );
        }
    }

    #[test]
    fn cancelled_and_superseded_cleanup_never_report_success() {
        assert_eq!(
            DictationOutcome::Cancelled.terminal_reason(),
            TerminalReason::Cancelled
        );
        assert_eq!(
            DictationOutcome::Superseded.terminal_reason(),
            TerminalReason::Cancelled
        );
        assert_eq!(
            DictationOutcome::NoSpeech.terminal_reason(),
            TerminalReason::Completed
        );
    }
}
