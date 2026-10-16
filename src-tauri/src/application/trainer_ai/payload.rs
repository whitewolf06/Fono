//! Numeric coaching data never embeds source keys, repeated words or findings.
use crate::{
    application::trainer_report::{TrainerAiContext, TrainerAiContextLimits, TrainerSnapshot},
    error::{AppError, AppResult},
    types::{DictationHistoryEntry, SpeechLlmDataScope},
};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::OnceLock};

pub(crate) const PROMPT_VERSION: &str = "trainer-period-ai-2";
pub(crate) const MAX_PAYLOAD_BYTES: usize = 32_768;
pub(crate) const MAX_PATTERNS: usize = 40;
pub(crate) const MAX_EXAMPLES_PER_PATTERN: usize = 2;
pub(crate) const MAX_SHARED_SESSIONS: usize = 12;
pub(crate) const MAX_SHARED_CHARS: usize = 3000;

pub(crate) struct PreparedPayload {
    pub input: String,
    pub context: TrainerAiContext,
}

fn word_count(text: &str) -> usize {
    text.split(|character: char| !character.is_alphabetic())
        .filter(|word| !word.is_empty())
        .count()
}

/// Keep Unicode lexical words intact, even when a UTF-8 budget cuts a prefix.
fn prefix(text: &str, maximum_characters: usize) -> &str {
    static WORD: OnceLock<regex::Regex> = OnceLock::new();
    // This fixed expression is compiled and exercised by the payload tests;
    // neither the regex syntax nor its options come from user input.
    let words = WORD.get_or_init(|| {
        regex::Regex::new(r"[\p{L}\p{M}\p{N}_]+")
            .expect("fixed Unicode lexical-word expression is valid")
    });
    let mut end = text
        .char_indices()
        .nth(maximum_characters)
        .map_or(text.len(), |(index, _)| index);
    if end < text.len() {
        for word in words.find_iter(text) {
            if word.start() < end && word.end() > end {
                end = word.start();
                break;
            }
            if word.start() >= end {
                break;
            }
        }
    }
    &text[..end]
}

fn fits(payload: &Value) -> AppResult<bool> {
    Ok(serde_json::to_vec(payload)?.len() <= MAX_PAYLOAD_BYTES)
}

fn array_field<'a>(value: &'a mut Value, key: &str) -> AppResult<&'a mut Vec<Value>> {
    value
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::Internal("Некорректная структура контекста тренера".into()))
}

fn replace_last_pattern(payload: &mut Value, pattern: Value) -> AppResult<()> {
    let last = array_field(payload, "findings")?
        .last_mut()
        .ok_or_else(|| {
            AppError::Internal("В контексте тренера отсутствует текущий паттерн".into())
        })?;
    *last = pattern;
    Ok(())
}

fn push_text(payload: &mut Value, text: &str) -> AppResult<()> {
    let sample = payload
        .get_mut("textSample")
        .ok_or_else(|| AppError::Internal("В контексте тренера отсутствует выборка".into()))?;
    let texts = array_field(sample, "originalTexts")?;
    texts.push(json!(text));
    sample["sharedSessions"] = json!(texts.len());
    Ok(())
}

fn pop_text(payload: &mut Value) -> AppResult<()> {
    let sample = payload
        .get_mut("textSample")
        .ok_or_else(|| AppError::Internal("В контексте тренера отсутствует выборка".into()))?;
    let texts = array_field(sample, "originalTexts")?;
    texts.pop();
    sample["sharedSessions"] = json!(texts.len());
    Ok(())
}

/// Preflight and HTTP submission use this identical, bounded JSON builder.
/// The byte budget covers period data, not system instructions or a model's window.
pub(crate) fn prepare(
    snapshot: &TrainerSnapshot,
    sources: &[DictationHistoryEntry],
    scope: SpeechLlmDataScope,
) -> AppResult<PreparedPayload> {
    // Rank and kind are stable, nonpersonal references. A repetition's local
    // key/label contains a user's word and therefore is excluded by default.
    let ranks: Vec<_> = snapshot
        .terms
        .iter()
        .take(MAX_PATTERNS)
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
            "contextByteLimit": MAX_PAYLOAD_BYTES,
            "confidenceLimit": "Local text heuristics; transcription accuracy and causes are not established. Examples and original texts may be omitted or shortened by Fono limits; aggregate counts cover the whole included period."
        },
        "counts": snapshot.counts, "density": snapshot.density,
        "daily": snapshot.daily, "patternRanks": ranks
    });
    let shared = scope != SpeechLlmDataScope::MetricsOnly;
    if shared {
        payload["findings"] = json!([]);
    }
    if scope == SpeechLlmDataScope::OriginalText {
        payload["textSample"] = json!({
            "sharedSessions": 0, "availableSessions": sources.len(),
            "maxCharactersPerSession": MAX_SHARED_CHARS, "originalTexts": []
        });
    }
    if !fits(&payload)? {
        return Err(AppError::Config(
            "Сводка периода превышает лимит контекста Fono".into(),
        ));
    }
    let mut context = TrainerAiContext {
        data_scope: scope,
        payload_bytes: 0,
        payload_byte_limit: MAX_PAYLOAD_BYTES,
        reduced: false,
        metrics_sessions: snapshot.included_sessions as usize,
        metrics_words: snapshot.total_words as usize,
        available_sessions: 0,
        shared_sessions: 0,
        omitted_sessions: 0,
        shortened_sessions: 0,
        available_words: 0,
        shared_words: 0,
        omitted_words: 0,
        available_characters: 0,
        shared_characters: 0,
        omitted_characters: 0,
        patterns_available: snapshot.terms.len(),
        patterns_shared: ranks.len(),
        examples_available: 0,
        examples_shared: 0,
        limits: TrainerAiContextLimits {
            max_shared_sessions: MAX_SHARED_SESSIONS,
            max_characters_per_session: MAX_SHARED_CHARS,
            max_patterns: MAX_PATTERNS,
            max_examples_per_pattern: MAX_EXAMPLES_PER_PATTERN,
        },
    };
    let mut available_example_sessions = HashSet::new();
    let mut shared_example_sessions = HashSet::new();
    let mut omitted_example_sessions = HashSet::new();
    if shared {
        context.patterns_shared = 0;
        for term in &snapshot.terms {
            context.examples_available += term.examples.len();
            for example in &term.examples {
                available_example_sessions.insert(example.session_id.as_str());
                if scope == SpeechLlmDataScope::Findings {
                    context.available_characters += example.fragment.chars().count();
                    context.available_words += word_count(&example.fragment);
                }
            }
        }
        for (term_index, term) in snapshot.terms.iter().enumerate() {
            let mut pattern = json!({
                "rank": term_index + 1,
                "kind": term.kind, "label": term.label, "count": term.count, "examples": []
            });
            let mut pattern_included = false;
            if term_index < MAX_PATTERNS {
                let items = array_field(&mut payload, "findings")?;
                items.push(pattern.clone());
                if fits(&payload)? {
                    pattern_included = true;
                    context.patterns_shared += 1;
                } else {
                    array_field(&mut payload, "findings")?.pop();
                }
            }
            for (example_index, example) in term.examples.iter().enumerate() {
                let mut included = false;
                if pattern_included && example_index < MAX_EXAMPLES_PER_PATTERN {
                    array_field(&mut pattern, "examples")?.push(json!(example.fragment));
                    replace_last_pattern(&mut payload, pattern.clone())?;
                    if fits(&payload)? {
                        included = true;
                        context.examples_shared += 1;
                        shared_example_sessions.insert(example.session_id.as_str());
                        if scope == SpeechLlmDataScope::Findings {
                            context.shared_characters += example.fragment.chars().count();
                            context.shared_words += word_count(&example.fragment);
                        }
                    } else {
                        array_field(&mut pattern, "examples")?.pop();
                        replace_last_pattern(&mut payload, pattern.clone())?;
                    }
                }
                if !included {
                    omitted_example_sessions.insert(example.session_id.as_str());
                }
            }
        }
    }
    if scope == SpeechLlmDataScope::Findings {
        context.available_sessions = available_example_sessions.len();
        context.shared_sessions = shared_example_sessions.len();
        context.shortened_sessions = shared_example_sessions
            .intersection(&omitted_example_sessions)
            .count();
    }
    if scope == SpeechLlmDataScope::OriginalText {
        for source in sources {
            if let Some(text) = source
                .original_text
                .as_deref()
                .filter(|text| !text.is_empty())
            {
                context.available_sessions += 1;
                context.available_characters += text.chars().count();
                context.available_words += word_count(text);
            }
        }
        for source in sources.iter().take(MAX_SHARED_SESSIONS) {
            let Some(text) = source
                .original_text
                .as_deref()
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            let mut low = 0;
            let mut high = text.chars().count().min(MAX_SHARED_CHARS);
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                push_text(&mut payload, prefix(text, middle))?;
                let included = fits(&payload)?;
                pop_text(&mut payload)?;
                if included {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            let excerpt = prefix(text, low);
            if excerpt.is_empty() {
                continue;
            }
            push_text(&mut payload, excerpt)?;
            context.shared_sessions += 1;
            context.shared_characters += excerpt.chars().count();
            context.shared_words += word_count(excerpt);
            if excerpt.len() < text.len() {
                context.shortened_sessions += 1;
            }
        }
    }
    context.omitted_sessions = context
        .available_sessions
        .saturating_sub(context.shared_sessions);
    context.omitted_words = context.available_words.saturating_sub(context.shared_words);
    context.omitted_characters = context
        .available_characters
        .saturating_sub(context.shared_characters);
    context.reduced = context.patterns_shared < context.patterns_available
        || context.examples_shared < context.examples_available
        || context.omitted_sessions > 0
        || context.shortened_sessions > 0;
    let input = serde_json::to_string(&payload)?;
    context.payload_bytes = input.len();
    debug_assert!(context.payload_bytes <= MAX_PAYLOAD_BYTES);
    Ok(PreparedPayload { input, context })
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
            history_limit: None,
            coverage_start: Some("2026-10-12T00:00:00Z".into()),
            retention_days: 30,
        }
    }
    #[test]
    fn default_period_request_has_no_source_words_labels_fragments_or_ids() {
        let prepared = prepare(&snapshot(), &[], SpeechLlmDataScope::MetricsOnly).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
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
        assert_eq!(prepared.context.shared_sessions, 0);
        assert_eq!(prepared.context.shared_words, 0);
        assert_eq!(prepared.context.shared_characters, 0);
        assert_eq!(prepared.context.metrics_sessions, 3);
        assert_eq!(prepared.context.metrics_words, 100);
        assert_eq!(prepared.context.payload_bytes, prepared.input.len());
        let metadata = serde_json::to_string(&prepared.context).unwrap();
        assert!(!metadata.contains("private-session-id"));
        assert!(!metadata.contains("СекретНимб"));
    }
    #[test]
    fn explicitly_shared_findings_do_not_share_original_transcripts() {
        let prepared = prepare(&snapshot(), &[], SpeechLlmDataScope::Findings).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        assert!(payload["findings"].is_array());
        assert!(payload.get("textSample").is_none());
        assert!(payload["patternRanks"][0].get("label").is_none());
        assert_eq!(prepared.context.available_sessions, 1);
        assert_eq!(prepared.context.shared_sessions, 1);
        assert_eq!(prepared.context.examples_shared, 1);
        assert_eq!(prepared.context.shared_words, 2);
        assert_eq!(
            prepared.context.shared_characters,
            "СекретНимб СекретНимб".chars().count()
        );
        assert!(!prepared.context.reduced);
    }

    fn source(index: usize, text: &str) -> DictationHistoryEntry {
        DictationHistoryEntry {
            id: format!("source-{index}"),
            created_at: "2026-10-12T12:00:00Z".parse().unwrap(),
            text: "finished result".into(),
            device: None,
            metadata: None,
            analytics_included: true,
            original_text: Some(text.into()),
            processing: None,
            analysis_status: crate::types::DictationAnalysisStatus::Ready,
            analysis: Some(crate::speech_metrics::analyze_russian_speech(text)),
            analysis_error: None,
            recommendation_status: crate::types::DictationAnalysisStatus::Pending,
            recommendation: None,
            recommendation_error: None,
        }
    }

    #[test]
    fn original_metadata_matches_sent_sessions_words_and_unicode_characters() {
        let long_text = "ёжик ".repeat(601);
        let mut sources = vec![source(0, &long_text)];
        sources.extend((1..13).map(|index| source(index, "Одна мысль.")));
        let prepared = prepare(&snapshot(), &sources, SpeechLlmDataScope::OriginalText).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        let texts = payload["textSample"]["originalTexts"].as_array().unwrap();
        assert_eq!(texts.len(), MAX_SHARED_SESSIONS);
        assert_eq!(prepared.context.shared_sessions, texts.len());
        assert_eq!(prepared.context.available_sessions, 13);
        assert_eq!(prepared.context.omitted_sessions, 1);
        assert_eq!(prepared.context.shortened_sessions, 1);
        assert_eq!(texts[0].as_str().unwrap().chars().count(), 3000);
        let sent_chars: usize = texts
            .iter()
            .map(|text| text.as_str().unwrap().chars().count())
            .sum();
        let sent_words: usize = texts
            .iter()
            .map(|text| word_count(text.as_str().unwrap()))
            .sum();
        assert_eq!(prepared.context.shared_characters, sent_chars);
        assert_eq!(prepared.context.shared_words, sent_words);
        assert_eq!(
            prepared.context.omitted_characters,
            long_text.chars().count() + 12 * "Одна мысль.".chars().count() - sent_chars
        );
        assert!(prepared.context.reduced);
    }

    #[test]
    fn total_utf8_budget_preserves_all_daily_metrics_and_reports_shortened_or_omitted_texts() {
        let text = "слово😀 \"\n".repeat(1000);
        let sources: Vec<_> = (0..12).map(|index| source(index, &text)).collect();
        let mut snapshot = snapshot();
        snapshot.days = 90;
        snapshot.daily = (0..90)
            .map(|index| TrainerDay {
                date: format!("day-{index}"),
                ..snapshot.daily[0].clone()
            })
            .collect();
        let prepared = prepare(&snapshot, &sources, SpeechLlmDataScope::OriginalText).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        assert_eq!(payload["daily"].as_array().unwrap().len(), 90);
        assert_eq!(payload["counts"], json!(snapshot.counts));
        assert_eq!(
            prepared.context.metrics_words,
            snapshot.total_words as usize
        );
        assert_eq!(prepared.input.len(), prepared.context.payload_bytes);
        assert!(prepared.input.len() <= MAX_PAYLOAD_BYTES);
        let texts = payload["textSample"]["originalTexts"].as_array().unwrap();
        assert!(!texts.is_empty());
        assert!(texts.len() < sources.len());
        assert_eq!(prepared.context.shortened_sessions, texts.len());
        assert_eq!(
            prepared.context.omitted_sessions,
            sources.len() - texts.len()
        );
        assert_eq!(
            prepared.context.shared_characters,
            texts
                .iter()
                .map(|text| text.as_str().unwrap().chars().count())
                .sum::<usize>()
        );
        assert_eq!(
            prepared.context.shared_words,
            texts
                .iter()
                .map(|text| word_count(text.as_str().unwrap()))
                .sum::<usize>()
        );
    }

    #[test]
    fn whole_word_prefixes_do_not_invent_partial_words_or_split_unicode_sequences() {
        assert_eq!(prefix("Начало сверхдлинноеслово", 15), "Начало ");
        assert_eq!(prefix("Начало cafe\u{301}123_end", 11), "Начало ");
        assert_eq!(prefix("😀 слово", 1), "😀");
        assert_eq!(prefix("длинноеслово", 3), "");
        assert_eq!(prefix("ровно", 5), "ровно");
        assert_eq!(word_count("Ёжик😀 42 café е\u{301} "), 3);
        let text = format!("{}сверхдлинноеслово", "я ".repeat(1497));
        let prepared = prepare(
            &snapshot(),
            &[source(0, &text)],
            SpeechLlmDataScope::OriginalText,
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        let excerpt = payload["textSample"]["originalTexts"][0].as_str().unwrap();
        assert!(text.starts_with(excerpt));
        assert_eq!(excerpt, "я ".repeat(1497));
        assert_eq!(prepared.context.shared_words, 1497);
        assert_eq!(prepared.context.omitted_words, 1);
    }

    #[test]
    fn pattern_and_example_caps_are_visible_and_oversized_labels_cannot_exceed_the_budget() {
        let mut snapshot = snapshot();
        let original = snapshot.terms[0].clone();
        snapshot.terms = (0..41)
            .map(|index| {
                let mut term = original.clone();
                term.label = if index == 0 {
                    "длинноеслово".repeat(MAX_PAYLOAD_BYTES)
                } else {
                    format!("pattern-{index}")
                };
                term.examples = vec![original.examples[0].clone(); 3];
                term
            })
            .collect();
        let prepared = prepare(&snapshot, &[], SpeechLlmDataScope::Findings).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        assert_eq!(payload["patternRanks"].as_array().unwrap().len(), 40);
        assert_eq!(payload["findings"].as_array().unwrap().len(), 39);
        assert_eq!(payload["findings"][0]["rank"], 2);
        assert_eq!(
            payload["findings"][0]["rank"],
            payload["patternRanks"][1]["rank"]
        );
        assert_eq!(prepared.context.patterns_available, 41);
        assert_eq!(prepared.context.patterns_shared, 39);
        assert_eq!(prepared.context.examples_available, 123);
        assert_eq!(prepared.context.examples_shared, 78);
        assert_eq!(prepared.context.shortened_sessions, 1);
        assert_eq!(prepared.context.available_sessions, 1);
        assert_eq!(prepared.context.shared_sessions, 1);
        assert!(prepared.context.reduced);
        assert!(prepared.input.len() <= MAX_PAYLOAD_BYTES);
        assert!(!prepared.input.contains(&"длинноеслово".repeat(100)));
    }

    #[test]
    fn context_preparation_is_deterministic_and_does_not_mutate_its_inputs() {
        let snapshot = snapshot();
        let sources = vec![source(0, "Одна мысль и ещё мысль.")];
        let before = serde_json::to_value((&snapshot, &sources)).unwrap();
        for scope in [
            SpeechLlmDataScope::MetricsOnly,
            SpeechLlmDataScope::Findings,
            SpeechLlmDataScope::OriginalText,
        ] {
            let first = prepare(&snapshot, &sources, scope).unwrap();
            let actual = prepare(&snapshot, &sources, scope).unwrap();
            assert_eq!(first.input, actual.input);
            assert_eq!(first.context, actual.context);
        }
        assert_eq!(before, serde_json::to_value((&snapshot, &sources)).unwrap());
    }

    #[test]
    fn largest_numeric_values_for_ninety_days_and_forty_ranks_fit_without_losing_metrics() {
        let mut snapshot = snapshot();
        snapshot.days = 90;
        snapshot.included_sessions = u32::MAX;
        snapshot.total_words = u32::MAX;
        snapshot.counts = TrainerCounts {
            filler: u32::MAX,
            repetition: u32::MAX,
            self_correction: u32::MAX,
            unfinished: u32::MAX,
        };
        let day = TrainerDay {
            date: "2026-10-12".into(),
            sessions: u32::MAX,
            words: u32::MAX,
            counts: snapshot.counts.clone(),
            density: f32::MAX,
        };
        snapshot.daily = vec![day; 90];
        let mut term = snapshot.terms[0].clone();
        term.count = u32::MAX;
        term.affected_sessions = u32::MAX;
        term.density = f32::MAX;
        snapshot.terms = vec![term; 40];
        let prepared = prepare(&snapshot, &[], SpeechLlmDataScope::MetricsOnly).unwrap();
        let payload: Value = serde_json::from_str(&prepared.input).unwrap();
        assert!(prepared.input.len() <= MAX_PAYLOAD_BYTES);
        assert_eq!(payload["daily"].as_array().unwrap().len(), 90);
        assert_eq!(payload["patternRanks"].as_array().unwrap().len(), 40);
        assert_eq!(payload["counts"], json!(snapshot.counts));
        assert_eq!(payload["sample"]["totalWords"], u32::MAX);
        assert_eq!(payload["sample"]["contextByteLimit"], MAX_PAYLOAD_BYTES);
    }
}
