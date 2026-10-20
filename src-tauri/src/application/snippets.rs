//! Exact, local text templates. Imported strings are data, never expressions.
use crate::{
    error::{AppError, AppResult},
    types::Settings,
};
use fono_core::OperationCancellation;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_ENTRIES: usize = 64;
pub const MAX_TRIGGER_CHARS: usize = 80;
pub const MAX_TEXT_CHARS: usize = 8192;
pub const MAX_TOTAL_CHARS: usize = 128 * 1024;
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_EXPANDED_CHARS: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    snippets: Vec<Snippet>,
}

pub fn normalize(text: &str) -> String {
    text.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '…'))
        .to_lowercase()
        .replace('ё', "е")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn invalid(message: &str) -> AppError {
    AppError::Config(message.into())
}

pub fn validate(entries: &[Snippet]) -> AppResult<()> {
    if entries.len() > MAX_ENTRIES {
        return Err(invalid("Можно сохранить до 64 сниппетов."));
    }
    let mut triggers = HashSet::new();
    let mut total = 0;
    for entry in entries {
        let trigger = normalize(&entry.trigger);
        if trigger.is_empty()
            || entry.trigger.chars().count() > MAX_TRIGGER_CHARS
            || entry.trigger.contains(['?', '\n', '\r'])
            || entry.trigger.chars().any(char::is_control)
        {
            return Err(invalid(
                "Триггер должен быть непустой фразой до 80 символов без вопроса и переноса строки.",
            ));
        }
        if !triggers.insert(trigger) {
            return Err(invalid(
                "Триггеры сниппетов должны различаться после нормализации.",
            ));
        }
        if entry.text.trim().is_empty()
            || entry.text.chars().count() > MAX_TEXT_CHARS
            || entry
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(invalid(
                "Текст сниппета должен содержать от 1 до 8192 символов без управляющих символов.",
            ));
        }
        total += entry.text.chars().count() + entry.trigger.chars().count();
    }
    if total > MAX_TOTAL_CHARS {
        return Err(invalid(
            "Общий размер сниппетов превышает 128 тысяч символов.",
        ));
    }
    Ok(())
}

pub fn matching<'a>(settings: &'a Settings, text: &str) -> Option<&'a Snippet> {
    if !settings.snippets_enabled || validate(&settings.snippets).is_err() {
        return None;
    }
    let normalized = normalize(text);
    settings
        .snippets
        .iter()
        .find(|entry| entry.enabled && normalize(&entry.trigger) == normalized)
}

pub fn parse_json(json: &str) -> AppResult<Vec<Snippet>> {
    if json.len() > MAX_JSON_BYTES {
        return Err(invalid("JSON-файл превышает 1 МиБ."));
    }
    let document: Document = serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|_| invalid("Некорректный JSON сниппетов. Нужны version: 1 и массив snippets с trigger, text, enabled."))?;
    if document.version != 1 {
        return Err(invalid("Эта версия JSON сниппетов не поддерживается."));
    }
    validate(&document.snippets)?;
    Ok(document.snippets)
}

pub fn to_json(entries: &[Snippet]) -> AppResult<String> {
    validate(entries)?;
    Ok(serde_json::to_string_pretty(&Document {
        version: 1,
        snippets: entries.to_vec(),
    })?)
}

pub fn read_json_file(path: &std::path::Path) -> AppResult<Vec<Snippet>> {
    use std::io::Read;
    let file =
        std::fs::File::open(path).map_err(|_| invalid("Не удалось открыть JSON сниппетов."))?;
    if !file
        .metadata()
        .map_err(|_| invalid("Не удалось проверить JSON-файл."))?
        .is_file()
    {
        return Err(invalid("Выберите обычный JSON-файл."));
    }
    let mut content = String::new();
    file.take(MAX_JSON_BYTES as u64 + 1)
        .read_to_string(&mut content)
        .map_err(|_| invalid("Не удалось прочитать JSON в UTF-8."))?;
    parse_json(&content)
}

pub fn expand(
    entry: &Snippet,
    date: &str,
    time: &str,
    clipboard: impl FnOnce() -> AppResult<String>,
) -> AppResult<String> {
    let clipboard = if entry.text.contains("{clipboard}") {
        Some(clipboard()?)
    } else {
        None
    };
    let mut result = String::new();
    let mut rest = entry.text.as_str();
    // One pass over the original template. Clipboard contents cannot introduce
    // another variable, path lookup, command, script or network request.
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let replacement = [
            ("{date}", date),
            ("{time}", time),
            ("{clipboard}", clipboard.as_deref().unwrap_or("")),
        ]
        .into_iter()
        .find(|(token, _)| rest.starts_with(token));
        if let Some((token, value)) = replacement {
            result.push_str(value);
            rest = &rest[token.len()..];
        } else {
            result.push('{');
            rest = &rest[1..];
        }
        if result.chars().count() > MAX_EXPANDED_CHARS {
            return Err(invalid("Результат сниппета превышает 32 тысячи символов."));
        }
    }
    result.push_str(rest);
    if result.chars().count() > MAX_EXPANDED_CHARS {
        return Err(invalid("Результат сниппета превышает 32 тысячи символов."));
    }
    Ok(result)
}

pub async fn expand_native(
    entry: Snippet,
    cancellation: OperationCancellation,
) -> AppResult<String> {
    let activity = super::updates::activity::lease()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("Сниппет отменён.".into()));
        }
        let now = chrono::Local::now();
        let result = expand(
            &entry,
            &now.format("%d.%m.%Y").to_string(),
            &now.format("%H:%M").to_string(),
            || crate::injection::read_snippet_clipboard(cancellation.clone()),
        );
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("Сниппет отменён.".into()));
        }
        result
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось подготовить сниппет.".into()))?
}

pub fn archive_settings(settings: &Settings) -> Settings {
    let mut snapshot = settings.clone();
    snapshot.ai_mode = crate::types::AiMode::Off;
    snapshot.processing_translation_enabled = false;
    snapshot.processing_target_language = None;
    snapshot.personal_dictionary_enabled = false;
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(trigger: &str, text: &str) -> Snippet {
        Snippet {
            trigger: trigger.into(),
            text: text.into(),
            enabled: true,
        }
    }
    #[test]
    fn matches_only_the_entire_utterance_and_preserves_questions() {
        let mut settings = Settings {
            snippets_enabled: true,
            snippets: vec![entry("Моя подпись", "Дмитрий")],
            ..Settings::default()
        };
        assert!(matching(&settings, "  МОЯ   ПОДПИСЬ. ").is_some());
        for text in [
            "моя подпись?",
            "какая моя подпись",
            "моя подпись и дата",
            "это моя подпись",
        ] {
            assert!(matching(&settings, text).is_none());
        }
        settings.snippets[0].enabled = false;
        assert!(matching(&settings, "моя подпись").is_none());
        settings.snippets[0].enabled = true;
        settings.snippets_enabled = false;
        assert!(matching(&settings, "моя подпись").is_none());
        assert_eq!(settings.snippets.len(), 1);
    }
    #[test]
    fn variables_are_lazy_single_pass_and_read_clipboard_once() {
        let result = expand(
            &entry("подпись", "{date} {time}: {clipboard} {clipboard}"),
            "21.10.2026",
            "12:34",
            || Ok("{date}; $(run())".into()),
        )
        .unwrap();
        assert_eq!(
            result,
            "21.10.2026 12:34: {date}; $(run()) {date}; $(run())"
        );
        assert_eq!(
            expand(
                &entry("текст", "{unknown} javascript:alert(1)"),
                "",
                "",
                || panic!("clipboard read without variable")
            )
            .unwrap(),
            "{unknown} javascript:alert(1)"
        );
        assert!(expand(&entry("текст", "{clipboard}"), "", "", || Err(
            invalid("locked")
        ))
        .is_err());
    }
    #[test]
    fn import_is_versioned_bounded_and_never_executes_or_enables_the_feature() {
        let entries = vec![entry("текст", "<script>throw 1</script> {clipboard}")];
        assert_eq!(parse_json(&to_json(&entries).unwrap()).unwrap(), entries);
        for json in [
            r#"{"version":2,"snippets":[]}"#,
            r#"{"version":1,"snippets":[],"run":"cmd.exe"}"#,
            r#"{"version":1,"snippets":[{"trigger":"x","text":"ok","command":"cmd"}]}"#,
        ] {
            assert!(parse_json(json).is_err());
        }
        assert!(parse_json(&" ".repeat(MAX_JSON_BYTES + 1)).is_err());
        assert!(validate(&[entry("Ёж!", "x"), entry("еж.", "y")]).is_err());
        assert!(validate(&[entry("?", "x")]).is_err());
        assert!(validate(&[entry("x", &"a".repeat(MAX_TEXT_CHARS + 1))]).is_err());
    }
    #[test]
    fn legacy_settings_and_processing_metadata_keep_safe_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.snippets_enabled);
        assert!(settings.snippets.is_empty());
        let settings = Settings {
            ai_mode: crate::types::AiMode::Clean,
            personal_dictionary_enabled: true,
            processing_translation_enabled: true,
            ..settings
        };
        let snapshot = archive_settings(&settings);
        assert_eq!(snapshot.ai_mode, crate::types::AiMode::Off);
        assert!(!snapshot.personal_dictionary_enabled);
        assert!(!snapshot.processing_translation_enabled);
        assert_eq!(settings.ai_mode, crate::types::AiMode::Clean);
    }

    #[tokio::test]
    async fn cancelled_expansion_never_opens_the_clipboard_owner() {
        let cancel = OperationCancellation::default();
        cancel.cancel();
        assert!(matches!(
            expand_native(entry("буфер", "{clipboard}"), cancel).await,
            Err(AppError::Cancelled(_))
        ));
    }
}
