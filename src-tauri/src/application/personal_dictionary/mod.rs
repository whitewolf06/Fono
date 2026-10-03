//! Optional local canonical spelling for the final ordinary dictation result.
//! This is one deterministic replacement pass, never a model prompt or a
//! command recognizer. Original transcripts and dictionary entries stay intact.
mod replacements;
mod validation;

#[cfg(test)]
use crate::types::PersonalDictionaryEntry;
use crate::types::Settings;
pub use validation::validate;

pub const MAX_ENTRIES: usize = 128;
pub const MAX_SPOKEN_VARIANTS: usize = 8;
pub const MAX_PHRASE_CHARS: usize = 120;
pub const MAX_TOTAL_BYTES: usize = 32_768;

pub fn canonicalize_dictation(settings: &Settings, text: &str) -> String {
    if !settings.personal_dictionary_enabled
        || settings.personal_dictionary_entries.is_empty()
        || validate(&settings.personal_dictionary_entries).is_err()
    {
        return text.to_owned();
    }
    replacements::replace(text, &settings.personal_dictionary_entries)
}

fn normalize(phrase: &str) -> String {
    phrase
        .split_whitespace()
        .map(|part| {
            part.chars()
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_word(phrase: &str) -> bool {
    phrase.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AiMode, Transcript};

    fn entry(written: &str, spoken: &[&str]) -> PersonalDictionaryEntry {
        PersonalDictionaryEntry {
            written: written.into(),
            spoken: spoken.iter().map(|value| (*value).into()).collect(),
        }
    }

    fn settings(entries: Vec<PersonalDictionaryEntry>) -> Settings {
        Settings {
            personal_dictionary_enabled: true,
            personal_dictionary_entries: entries,
            ..Settings::default()
        }
    }

    #[test]
    fn disabled_keeps_entries_and_text_unchanged() {
        let mut settings = settings(vec![entry("Fono", &["фоно"])]);
        settings.personal_dictionary_enabled = false;
        assert_eq!(canonicalize_dictation(&settings, "фоно"), "фоно");
        assert_eq!(settings.personal_dictionary_entries.len(), 1);
    }

    #[test]
    fn respects_cyrillic_latin_identifier_and_combining_boundaries() {
        let settings = settings(vec![entry("Fono", &["фоно", "fono"])]);
        assert_eq!(
            canonicalize_dictation(
                &settings,
                "ФОНО, fono! микрофоно fonometer _fono fono2 fono² fono\u{301}"
            ),
            "Fono, Fono! микрофоно fonometer _fono fono2 fono² fono\u{301}"
        );
    }

    #[test]
    fn longest_phrase_wins_and_replacements_do_not_cascade() {
        let settings = settings(vec![
            entry("White", &["вайт"]),
            entry("WhiteLife", &["вайт лайф"]),
            entry("Fono", &["WhiteLife"]),
        ]);
        assert_eq!(
            canonicalize_dictation(&settings, "ВАЙТ\t  лайф и вайт"),
            "WhiteLife и White"
        );
        assert_eq!(canonicalize_dictation(&settings, "WhiteLife"), "Fono");
    }

    #[test]
    fn phrase_matching_keeps_punctuation_and_does_not_bridge_it() {
        let settings = settings(vec![entry("WhiteLife", &["вайт лайф"])]);
        assert_eq!(
            canonicalize_dictation(&settings, "(вайт лайф), вайт, лайф"),
            "(WhiteLife), вайт, лайф"
        );
    }

    #[test]
    fn ambiguous_and_corrupt_dictionaries_fail_closed_even_when_enabled() {
        let settings = settings(vec![entry("Fono", &["Фоно"]), entry("Other", &["фоно"])]);
        assert!(validate(&settings.personal_dictionary_entries).is_err());
        assert_eq!(canonicalize_dictation(&settings, "Фоно"), "Фоно");
        assert!(validate(&[entry("Fono", &["\nфоно"])]).is_err());
        assert!(validate(&[entry("Fono", &["..."])]).is_err());
        assert!(validate(&[entry(&"я".repeat(MAX_PHRASE_CHARS + 1), &["фоно"])]).is_err());
        assert!(validate(&vec![entry("Fono", &["фоно"]); MAX_ENTRIES + 1]).is_err());
        assert!(validate(&[entry("Fono", &["фоно", "ФОНО"])]).is_err());
    }

    #[test]
    fn replacement_is_independent_of_ai_mode_and_preserves_original_for_archive() {
        let original = Transcript {
            text: "Запусти фоно".into(),
            detected_language: Some("ru".into()),
            transcribe_secs: Some(0.1),
            audio_secs: Some(1.0),
            device: Some("CPU".into()),
        };
        let mut settings = settings(vec![entry("Fono", &["фоно"])]);
        for mode in [AiMode::Off, AiMode::Clean, AiMode::Format] {
            settings.ai_mode = mode;
            let result = canonicalize_dictation(&settings, &original.text);
            assert_eq!(result, "Запусти Fono");
            assert_eq!(original.text, "Запусти фоно");
        }
    }

    #[test]
    fn old_settings_default_off_and_disabled_dictionary_survives_serialization() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.personal_dictionary_enabled);
        assert!(old.personal_dictionary_entries.is_empty());
        let mut configured = settings(vec![entry("Fono", &["фоно"])]);
        configured.personal_dictionary_enabled = false;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&configured).unwrap()).unwrap();
        assert!(!restored.personal_dictionary_enabled);
        assert_eq!(restored.personal_dictionary_entries[0].written, "Fono");
    }
}
