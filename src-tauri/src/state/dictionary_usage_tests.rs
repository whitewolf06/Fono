use super::*;
use crate::{application::personal_dictionary::GlossarySnapshot, types::PersonalDictionaryEntry};

fn state() -> AppState {
    AppState {
        settings: Mutex::new(Settings {
            personal_dictionary_enabled: true,
            history_enabled: false,
            personal_dictionary_entries: vec![
                PersonalDictionaryEntry {
                    written: "Fono".into(),
                    spoken: vec!["фоно".into()],
                },
                PersonalDictionaryEntry {
                    written: "Whisper".into(),
                    spoken: vec!["виспер".into()],
                },
            ],
            ..Settings::default()
        }),
        dictation_paused: Mutex::new(false),
        settings_version: AtomicU64::new(1),
        dictionary_usage: Mutex::new(Default::default()),
        next_command_proposal_id: AtomicU64::new(1),
        pending_command_proposal: Mutex::new(None),
    }
}

fn hints(state: &AppState) -> Vec<String> {
    GlossarySnapshot::from_settings(&state.dictation_settings())
        .terms()
        .to_vec()
}

fn accept(state: &AppState, transcript: &str) {
    state.accepted_dictionary_usage(&state.dictation_settings(), transcript);
}

#[test]
fn accepted_uses_rank_by_dictation_frequency_then_recency_without_reading_history() {
    let state = state();
    assert!(!state.settings().history_enabled);
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
    accept(&state, "Виспер, виспер, WHISPER!");
    assert_eq!(hints(&state), ["Whisper", "Fono"]);
    // Repetition in one transcript contributes once, not three times.
    accept(&state, "фоно");
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
    accept(&state, "whisper");
    accept(&state, "whisper");
    accept(&state, "Fono");
    assert_eq!(hints(&state), ["Whisper", "Fono"]);
    accept(&state, "фоно");
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
}

#[test]
fn captured_priorities_are_frozen_and_do_not_reorder_renderer_entries() {
    let state = state();
    let captured = state.dictation_settings();
    accept(&state, "Whisper");
    assert_eq!(hints(&state), ["Whisper", "Fono"]);
    assert_eq!(
        GlossarySnapshot::from_settings(&captured).terms(),
        ["Fono", "Whisper"]
    );
    assert_eq!(
        state.settings().personal_dictionary_entries[0].written,
        "Fono"
    );
}

#[test]
fn frequent_term_outside_the_original_limit_enters_the_bounded_prompt() {
    let state = state();
    let mut settings = state.settings();
    settings.personal_dictionary_entries = (0..128)
        .map(|i| PersonalDictionaryEntry {
            written: format!("Term{i:03}"),
            spoken: vec![format!("spoken{i:03}")],
        })
        .collect();
    state.set_settings(settings);
    assert_eq!(hints(&state).len(), 32);
    assert!(!hints(&state).iter().any(|term| term == "Term127"));
    accept(&state, "spoken127");
    let terms = hints(&state);
    assert_eq!(terms[0], "Term127");
    assert_eq!(terms.len(), 32);
    assert!(terms.join(", ").len() <= fono_stt_protocol::prompt::MAX_BYTES);
}

#[test]
fn disabling_or_erasing_invalidates_late_updates_without_losing_entries() {
    let state = state();
    let old = state.dictation_settings();
    accept(&state, "Whisper");
    let mut settings = state.settings();
    settings.personal_dictionary_enabled = false;
    state.set_settings(settings.clone());
    state.accepted_dictionary_usage(&old, "Whisper");
    assert!(hints(&state).is_empty());
    assert_eq!(state.settings().personal_dictionary_entries.len(), 2);
    settings.personal_dictionary_enabled = true;
    state.set_settings(settings);
    state.accepted_dictionary_usage(&old, "Whisper");
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
    let old = state.dictation_settings();
    let mut settings = state.settings();
    let removed = settings.personal_dictionary_entries.pop().unwrap();
    state.set_settings(settings.clone());
    settings.personal_dictionary_entries.push(removed);
    state.set_settings(settings);
    state.accepted_dictionary_usage(&old, "Whisper");
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
}

#[test]
fn priorities_are_runtime_only_and_cannot_be_restored_or_forged_through_settings() {
    let state = state();
    accept(&state, "Whisper");
    let mut serialized = serde_json::to_value(state.dictation_settings()).unwrap();
    assert!(serialized.get("personal_dictionary_usage").is_none());
    serialized["personal_dictionary_usage"] = serde_json::json!({
        "epoch": 1, "priorities": {"whisper": {"dictations": 999, "last_used": 999}}
    });
    let incoming: Settings = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        GlossarySnapshot::from_settings(&incoming).terms(),
        ["Fono", "Whisper"]
    );
    state.accepted_dictionary_usage(&incoming, "Whisper");
    let restored = self::state();
    assert_eq!(hints(&restored), ["Fono", "Whisper"]);
    assert!(GlossarySnapshot::for_llm(&state.dictation_settings())
        .terms()
        .is_empty());
}

#[test]
fn usage_matching_respects_word_boundaries_and_the_bounded_tail() {
    let state = state();
    accept(&state, "_whisper whisper2 микровиспер whisper\u{301}");
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
    accept(&state, &format!("Fono {} (виспер)", "x".repeat(20_000)));
    assert_eq!(hints(&state), ["Whisper", "Fono"]);
    let state = self::state();
    accept(&state, &format!("{}Fono", "x".repeat(20_000)));
    assert_eq!(hints(&state), ["Fono", "Whisper"]);
}
