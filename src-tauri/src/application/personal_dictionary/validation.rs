use std::collections::BTreeSet;

use super::{
    has_word, normalize, MAX_ENTRIES, MAX_PHRASE_CHARS, MAX_SPOKEN_VARIANTS, MAX_TOTAL_BYTES,
};
use crate::{
    error::{AppError, AppResult},
    types::PersonalDictionaryEntry,
};

pub fn validate(entries: &[PersonalDictionaryEntry]) -> AppResult<()> {
    if entries.len() > MAX_ENTRIES {
        return invalid(format!(
            "В личном словаре может быть не больше {MAX_ENTRIES} записей."
        ));
    }
    let mut variants = BTreeSet::new();
    let mut bytes = 0usize;
    for (index, entry) in entries.iter().enumerate() {
        validate_phrase(&entry.written, index)?;
        bytes += entry.written.len();
        if entry.spoken.is_empty() || entry.spoken.len() > MAX_SPOKEN_VARIANTS {
            return invalid(format!(
                "Запись {}: укажите от 1 до {MAX_SPOKEN_VARIANTS} вариантов произношения.",
                index + 1
            ));
        }
        for spoken in &entry.spoken {
            validate_phrase(spoken, index)?;
            bytes += spoken.len();
            if !variants.insert(normalize(spoken)) {
                return invalid(format!(
                    "Запись {}: этот вариант произношения уже есть в словаре.",
                    index + 1
                ));
            }
        }
    }
    if bytes > MAX_TOTAL_BYTES {
        return invalid("Личный словарь превышает лимит 32 КБ.".into());
    }
    Ok(())
}

fn validate_phrase(value: &str, index: usize) -> AppResult<()> {
    if value.trim() != value
        || value.is_empty()
        || value.chars().count() > MAX_PHRASE_CHARS
        || value.chars().any(char::is_control)
        || !has_word(value)
    {
        return invalid(format!("Запись {}: фраза должна содержать буквы или цифры, не больше {MAX_PHRASE_CHARS} символов, без переносов строк и краевых пробелов.", index + 1));
    }
    Ok(())
}

fn invalid<T>(message: String) -> AppResult<T> {
    Err(AppError::Config(message))
}
