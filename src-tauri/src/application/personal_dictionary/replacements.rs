use super::normalize;
use crate::types::PersonalDictionaryEntry;

struct Unit {
    value: char,
    start: usize,
    end: usize,
}

struct Rule<'a> {
    phrase: Vec<char>,
    written: &'a str,
}

pub(super) fn replace(text: &str, entries: &[PersonalDictionaryEntry]) -> String {
    let units = normalized_units(text);
    let mut rules: Vec<_> = entries
        .iter()
        .flat_map(|entry| {
            entry.spoken.iter().map(|spoken| Rule {
                phrase: normalize(spoken).chars().collect(),
                written: entry.written.as_str(),
            })
        })
        .collect();
    rules.sort_by_key(|rule| std::cmp::Reverse(rule.phrase.len()));
    let mut output = String::with_capacity(text.len());
    let mut copied_until = 0usize;
    let mut cursor = 0usize;
    while cursor < units.len() {
        let start = units[cursor].start;
        // Lowercasing can expand one character. Never start within its expansion.
        if cursor > 0 && units[cursor - 1].start == start {
            cursor += 1;
            continue;
        }
        let found = rules.iter().find(|rule| {
            let end_cursor = cursor + rule.phrase.len();
            if end_cursor > units.len()
                || !units[cursor..end_cursor]
                    .iter()
                    .map(|unit| unit.value)
                    .eq(rule.phrase.iter().copied())
            {
                return false;
            }
            let end = units[end_cursor - 1].end;
            if end_cursor < units.len() && units[end_cursor].start < end {
                return false;
            }
            !text[..start].chars().next_back().is_some_and(is_word)
                && !text[end..].chars().next().is_some_and(is_word)
        });
        if let Some(rule) = found {
            let end_cursor = cursor + rule.phrase.len();
            output.push_str(&text[copied_until..start]);
            output.push_str(rule.written);
            copied_until = units[end_cursor - 1].end;
            cursor = end_cursor;
        } else {
            cursor += 1;
        }
    }
    output.push_str(&text[copied_until..]);
    output
}

fn normalized_units(text: &str) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    for (start, character) in text.char_indices() {
        let end = start + character.len_utf8();
        if character.is_whitespace() {
            if let Some(previous) = units.last_mut().filter(|unit| unit.value == ' ') {
                previous.end = end;
            } else {
                units.push(Unit {
                    value: ' ',
                    start,
                    end,
                });
            }
        } else {
            units.extend(
                character
                    .to_lowercase()
                    .map(|value| Unit { value, start, end }),
            );
        }
    }
    units
}

fn is_word(character: char) -> bool {
    static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let matcher = WORD.get_or_init(|| {
        regex::Regex::new(r"^[\pL\pN\pM\p{Pc}\x{200C}\x{200D}]$")
            .expect("constant Unicode word regex")
    });
    let mut buffer = [0u8; 4];
    matcher.is_match(character.encode_utf8(&mut buffer))
}
