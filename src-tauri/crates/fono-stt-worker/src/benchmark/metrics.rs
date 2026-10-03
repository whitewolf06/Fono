fn distance<T: PartialEq>(expected: &[T], actual: &[T]) -> usize {
    let mut previous: Vec<usize> = (0..=actual.len()).collect();
    let mut current = vec![0; actual.len() + 1];
    for (row, expected_item) in expected.iter().enumerate() {
        current[0] = row + 1;
        for (column, actual_item) in actual.iter().enumerate() {
            current[column + 1] = (previous[column] + usize::from(expected_item != actual_item))
                .min(current[column] + 1)
                .min(previous[column + 1] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[actual.len()]
}

fn normalized(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn errors(reference: &str, hypothesis: &str) -> serde_json::Value {
    let reference = normalized(reference);
    let hypothesis = normalized(hypothesis);
    let expected_words: Vec<_> = reference.split_whitespace().collect();
    let actual_words: Vec<_> = hypothesis.split_whitespace().collect();
    let expected_chars: Vec<_> = reference.chars().collect();
    let actual_chars: Vec<_> = hypothesis.chars().collect();
    serde_json::json!({
        "word_errors": distance(&expected_words, &actual_words), "reference_words": expected_words.len(),
        "wer": if expected_words.is_empty() { None } else { Some(distance(&expected_words, &actual_words) as f64 / expected_words.len() as f64) },
        "cer": if expected_chars.is_empty() { None } else { Some(distance(&expected_chars, &actual_chars) as f64 / expected_chars.len() as f64) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_count_unicode_characters_and_real_repetitions() {
        assert_eq!(errors("Да, да!", "да")["wer"], 0.5);
        assert_eq!(errors("Привет", "привет.")["cer"], 0.0);
        assert!(errors("", "слово")["wer"].is_null());
    }
}
