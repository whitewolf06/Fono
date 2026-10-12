//! Numeric coaching data never embeds source keys, repeated words or findings.
use crate::{
    application::trainer_report::TrainerSnapshot,
    types::{DictationHistoryEntry, SpeechLlmDataScope},
};
use serde_json::{json, Value};

pub(crate) const PROMPT_VERSION: &str = "trainer-period-ai-1";
pub(crate) const MAX_SHARED_SESSIONS: usize = 12;
pub(crate) const MAX_SHARED_CHARS: usize = 3000;

pub(crate) fn build(
    snapshot: &TrainerSnapshot,
    sources: &[DictationHistoryEntry],
    scope: SpeechLlmDataScope,
) -> Value {
    // Rank and kind are stable, nonpersonal references. A repetition's local
    // key/label contains a user's word and therefore is excluded by default.
    let ranks: Vec<_> = snapshot
        .terms
        .iter()
        .take(40)
        .enumerate()
        .map(|(index, term)| {
            json!({
                "rank": index + 1, "kind": term.kind, "count": term.count,
                "density": term.density, "affectedSessions": term.affected_sessions
            })
        })
        .collect();
    let mut payload = json!({
        "period": {"days": snapshot.days, "from": snapshot.from, "to": snapshot.to, "timezone": "UTC"},
        "rulesVersion": snapshot.rules_version,
        "sample": {
            "includedSessions": snapshot.included_sessions, "totalWords": snapshot.total_words,
            "excludedSessions": snapshot.excluded_sessions, "pendingSessions": snapshot.pending_sessions,
            "unavailableSessions": snapshot.unavailable_sessions, "historyLimit": snapshot.history_limit,
            "retentionDays": snapshot.retention_days, "coverageStart": snapshot.coverage_start,
            "sufficientSample": !snapshot.insufficient_data,
            "rankedPatternsIncluded": ranks.len(), "rankedPatternsTotal": snapshot.terms.len(),
            "confidenceLimit": "Local text heuristics; transcription accuracy and causes are not established."
        },
        "counts": snapshot.counts, "density": snapshot.density,
        "daily": snapshot.daily, "patternRanks": ranks
    });
    if scope != SpeechLlmDataScope::MetricsOnly {
        let patterns: Vec<_> = snapshot.terms.iter().take(40).map(|term| json!({
            "kind": term.kind, "label": term.label, "count": term.count,
            "examples": term.examples.iter().take(2)
                .map(|example| example.fragment.chars().take(300).collect::<String>()).collect::<Vec<_>>()
        })).collect();
        payload["findings"] = json!(patterns);
    }
    if scope == SpeechLlmDataScope::OriginalText {
        let texts: Vec<_> = sources
            .iter()
            .take(MAX_SHARED_SESSIONS)
            .filter_map(|source| {
                source
                    .original_text
                    .as_deref()
                    .map(|text| text.chars().take(MAX_SHARED_CHARS).collect::<String>())
            })
            .collect();
        payload["textSample"] = json!({
            "sharedSessions": texts.len(), "availableSessions": sources.len(),
            "maxCharactersPerSession": MAX_SHARED_CHARS, "originalTexts": texts
        });
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::trainer_report::{
        TrainerCounts, TrainerDay, TrainerExample, TrainerTerm,
    };

    fn snapshot() -> TrainerSnapshot {
        TrainerSnapshot {
            fingerprint: "source-fingerprint".into(),
            rules_version: "speech-patterns-1".into(),
            days: 7,
            from: "2026-10-06T00:00:00Z".into(),
            to: "2026-10-13T00:00:00Z".into(),
            generated_at: "2026-10-12T12:00:00Z".into(),
            included_sessions: 3,
            excluded_sessions: 0,
            pending_sessions: 0,
            unavailable_sessions: 0,
            total_words: 100,
            counts: TrainerCounts {
                filler: 0,
                repetition: 1,
                self_correction: 0,
                unfinished: 0,
            },
            density: 1.0,
            terms: vec![TrainerTerm {
                key: "repetition:секретнимб".into(),
                label: "секретнимб".into(),
                kind: crate::types::SpeechFindingKind::Repetition,
                count: 1,
                density: 1.0,
                affected_sessions: 1,
                session_ids: vec!["private-session-id".into()],
                examples: vec![TrainerExample {
                    session_id: "private-session-id".into(),
                    fragment: "СекретНимб СекретНимб".into(),
                }],
            }],
            daily: vec![TrainerDay {
                date: "2026-10-12".into(),
                sessions: 3,
                words: 100,
                counts: TrainerCounts {
                    filler: 0,
                    repetition: 1,
                    self_correction: 0,
                    unfinished: 0,
                },
                density: 1.0,
            }],
            session_ids: vec!["private-session-id".into()],
            insufficient_data: false,
            history_limit: 200,
            coverage_start: Some("2026-10-12T00:00:00Z".into()),
            retention_days: 30,
        }
    }
    #[test]
    fn default_period_request_has_no_source_words_labels_fragments_or_ids() {
        let payload = build(&snapshot(), &[], SpeechLlmDataScope::MetricsOnly);
        let serialized = payload.to_string();
        for forbidden in [
            "секретнимб",
            "СекретНимб",
            "private-session-id",
            "fragment",
            "label",
            "originalTexts",
            "sessionIds",
        ] {
            assert!(!serialized.contains(forbidden), "unexpected {forbidden}");
        }
        assert!(payload.get("findings").is_none());
        assert_eq!(payload["counts"]["repetition"], 1);
    }
    #[test]
    fn explicitly_shared_findings_do_not_share_original_transcripts() {
        let payload = build(&snapshot(), &[], SpeechLlmDataScope::Findings);
        assert!(payload["findings"].is_array());
        assert!(payload.get("textSample").is_none());
        assert!(payload["patternRanks"][0].get("label").is_none());
    }
}
