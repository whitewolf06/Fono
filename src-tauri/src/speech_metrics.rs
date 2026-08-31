//! Pure local rules for explainable Russian speech metrics.

use crate::types::{SpeechFinding, SpeechFindingKind, SpeechSessionAnalysis};

const FILLER_WORDS: &[&str] = &["ну", "вот", "типа", "короче", "значит", "ээ", "эм", "мм"];
const FILLER_PHRASES: &[&[&str]] = &[&["как", "бы"]];
const SELF_CORRECTION_PHRASES: &[&[&str]] = &[
    &["то", "есть"],
    &["вернее"],
    &["точнее"],
    &["в", "смысле"],
    &["ой"],
];

#[derive(Debug, Clone)]
struct WordToken {
    source: String,
    normalized: String,
}

pub fn analyze_russian_speech(text: &str) -> SpeechSessionAnalysis {
    let words = tokenize(text);
    let mut findings = Vec::new();
    let filler_count = detect_fillers(&words, &mut findings);
    let repetition_count = detect_repetitions(&words, &mut findings);
    let self_correction_count = detect_self_corrections(&words, &mut findings);
    let unfinished_count = detect_unfinished(text, &words, &mut findings);
    let word_count = words.len() as u32;
    let filler_density_per_100_words = if word_count == 0 {
        0.0
    } else {
        (filler_count as f32 * 100.0) / word_count as f32
    };

    SpeechSessionAnalysis {
        word_count,
        filler_count,
        filler_density_per_100_words,
        repetition_count,
        self_correction_count,
        unfinished_count,
        findings,
    }
}

fn tokenize(text: &str) -> Vec<WordToken> {
    text.split(|character: char| !character.is_alphabetic())
        .filter(|word| !word.is_empty())
        .map(|word| WordToken {
            source: word.to_owned(),
            normalized: word.to_lowercase(),
        })
        .collect()
}

fn detect_fillers(words: &[WordToken], findings: &mut Vec<SpeechFinding>) -> u32 {
    let mut count = 0;
    let mut index = 0;
    while index < words.len() {
        if let Some(length) = phrase_length_at(words, index, FILLER_PHRASES) {
            findings.push(finding(
                SpeechFindingKind::Filler,
                "Слово-паразит",
                words,
                index,
                length,
            ));
            count += 1;
            index += length;
        } else if FILLER_WORDS.contains(&words[index].normalized.as_str()) {
            findings.push(finding(
                SpeechFindingKind::Filler,
                "Слово-паразит",
                words,
                index,
                1,
            ));
            count += 1;
            index += 1;
        } else {
            index += 1;
        }
    }
    count
}

fn detect_repetitions(words: &[WordToken], findings: &mut Vec<SpeechFinding>) -> u32 {
    let mut count = 0;
    for (index, pair) in words.windows(2).enumerate() {
        if pair[0].normalized == pair[1].normalized {
            findings.push(finding(
                SpeechFindingKind::Repetition,
                "Повтор слова",
                words,
                index,
                2,
            ));
            count += 1;
        }
    }
    count
}

fn detect_self_corrections(words: &[WordToken], findings: &mut Vec<SpeechFinding>) -> u32 {
    let mut count = 0;
    for index in 0..words.len() {
        if let Some(length) = phrase_length_at(words, index, SELF_CORRECTION_PHRASES) {
            findings.push(finding(
                SpeechFindingKind::SelfCorrection,
                "Маркер самопоправки",
                words,
                index,
                length,
            ));
            count += 1;
        }
    }
    count
}

fn detect_unfinished(text: &str, words: &[WordToken], findings: &mut Vec<SpeechFinding>) -> u32 {
    let trimmed = text.trim_end();
    if words.is_empty()
        || !(trimmed.ends_with('…') || trimmed.ends_with("...") || trimmed.ends_with('-'))
    {
        return 0;
    }

    let last_index = words.len() - 1;
    findings.push(finding(
        SpeechFindingKind::Unfinished,
        "Незавершённый фрагмент",
        words,
        last_index,
        1,
    ));
    1
}

fn phrase_length_at(words: &[WordToken], index: usize, phrases: &[&[&str]]) -> Option<usize> {
    phrases.iter().find_map(|phrase| {
        let candidate = words.get(index..index + phrase.len())?;
        candidate
            .iter()
            .zip(phrase.iter())
            .all(|(word, expected)| word.normalized == *expected)
            .then_some(phrase.len())
    })
}

fn finding(
    kind: SpeechFindingKind,
    label: &str,
    words: &[WordToken],
    start: usize,
    length: usize,
) -> SpeechFinding {
    let fragment = words[start..start + length]
        .iter()
        .map(|word| word.source.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    SpeechFinding {
        kind,
        label: label.to_owned(),
        fragment,
        start_word: start as u32,
        end_word: (start + length) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::analyze_russian_speech;
    use crate::types::SpeechFindingKind;

    #[test]
    fn recognizes_local_russian_patterns_with_explainable_fragments() {
        let analysis =
            analyze_russian_speech("Ну, как бы я я хотел, то есть, точнее объяснить это...");

        assert_eq!(analysis.word_count, 11);
        assert_eq!(analysis.filler_count, 2);
        assert_eq!(analysis.repetition_count, 1);
        assert_eq!(analysis.self_correction_count, 2);
        assert_eq!(analysis.unfinished_count, 1);
        assert!(analysis
            .findings
            .iter()
            .any(|finding| finding.kind == SpeechFindingKind::Repetition
                && finding.fragment == "я я"));
    }

    #[test]
    fn empty_transcript_has_no_metrics_or_findings() {
        let analysis = analyze_russian_speech("  ");

        assert_eq!(analysis.word_count, 0);
        assert_eq!(analysis.filler_density_per_100_words, 0.0);
        assert!(analysis.findings.is_empty());
    }
}
