/// Возвращает действие, явно продиктованное после wake phrase.
///
/// Намеренно не пытаемся угадывать команды из обычного текста: системные
/// действия должны требовать явного префикса «команда». Результат в нижнем
/// регистре безопасен для текущего сопоставления окон и приложений.
#[cfg(feature = "wake-word")]
pub(crate) fn extract_wake_command(transcript: &str) -> Option<String> {
    let normalized = transcript.trim().to_lowercase();
    for prefix in ["команда", "выполни команду", "command"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let command = rest
                .trim_start_matches(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | ',' | '.' | '-')
                })
                .trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }
    None
}

/// Removes only a leading wake phrase from the main dictation transcript.
/// Audio pre-roll intentionally preserves the first user words; this textual
/// guard prevents the detector phrase from leaking into the injected result.
#[cfg(feature = "wake-word")]
pub(crate) fn strip_leading_wake_phrase(transcript: &str, configured_phrase: &str) -> String {
    let trimmed = transcript.trim_start();
    let count = configured_phrase.split_whitespace().count();
    let words: Vec<&str> = trimmed.split_whitespace().take(count).collect();
    if count == 0 || words.len() < count {
        return trimmed.to_string();
    }
    let normalize_word = |word: &str| {
        word.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let expected: Vec<String> = configured_phrase
        .split_whitespace()
        .map(normalize_word)
        .collect();
    let matched = expected
        .iter()
        .zip(&words)
        .all(|(expected, actual)| !expected.is_empty() && expected == &normalize_word(actual));
    if !matched {
        return trimmed.to_string();
    }

    let mut rest = trimmed;
    for word in words {
        rest = rest.strip_prefix(word).unwrap_or(rest);
        rest = rest.trim_start();
    }
    rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | ',' | '.' | '-'))
        .trim_start()
        .to_string()
}

#[cfg(test)]
mod wake_command_tests {
    use super::{extract_wake_command, strip_leading_wake_phrase};

    #[test]
    fn extracts_only_explicit_command_prefixes() {
        assert_eq!(
            extract_wake_command("Команда: открой Telegram"),
            Some("открой telegram".to_string())
        );
        assert_eq!(
            extract_wake_command("command louder"),
            Some("louder".to_string())
        );
        assert_eq!(extract_wake_command("открой Telegram"), None);
    }

    #[test]
    fn removes_only_leading_wake_phrase_from_dictation() {
        assert_eq!(
            strip_leading_wake_phrase("ЭЙ ФОНО, напиши привет", "Эй Фоно"),
            "напиши привет"
        );
        assert_eq!(
            strip_leading_wake_phrase("эй фон open telegram", "Эй Фоно"),
            "эй фон open telegram"
        );
        assert_eq!(
            strip_leading_wake_phrase("просто Эй Фоно внутри фразы", "Эй Фоно"),
            "просто Эй Фоно внутри фразы"
        );
    }
}
