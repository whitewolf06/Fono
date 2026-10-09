/// Exact whole-word matching; no fuzzy/edit-distance expansion for activation.
pub fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c == 'ё' {
                'е'
            } else if c.is_alphanumeric() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
#[cfg(any(feature = "sherpa-wake", test))]
pub fn phrase_range(text: &str, phrase: &str) -> Option<(usize, usize)> {
    text.match_indices(phrase).find_map(|(start, _)| {
        let end = start + phrase.len();
        let left = start == 0 || text[..start].ends_with(' ');
        let right = end == text.len() || text[end..].starts_with(' ');
        (left && right).then_some((start, end))
    })
}
#[cfg(any(feature = "sherpa-wake", test))]
pub struct PhraseMatcher {
    phrase: String,
    stable_samples: u64,
    confirmations: u8,
    candidate: Option<(usize, u64, u8)>,
}
#[cfg(any(feature = "sherpa-wake", test))]
impl PhraseMatcher {
    pub fn new(phrase: &str, stability_ms: u64, confirmations: u8, rate: u32) -> Self {
        Self {
            phrase: normalize(phrase),
            stable_samples: stability_ms * rate as u64 / 1000,
            confirmations: confirmations.max(2),
            candidate: None,
        }
    }
    pub fn observe(&mut self, text: &str, sample: u64) -> Option<usize> {
        let text = normalize(text);
        let Some((start, end)) = phrase_range(&text, &self.phrase) else {
            self.candidate = None;
            return None;
        };
        let (candidate_start, since, count) = self.candidate.get_or_insert((start, sample, 0));
        if *candidate_start != start {
            *candidate_start = start;
            *since = sample;
            *count = 0;
        }
        *count = count.saturating_add(1);
        (*count >= self.confirmations && sample.saturating_sub(*since) >= self.stable_samples)
            .then_some(end)
    }
    pub fn reset(&mut self) {
        self.candidate = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avoids_partial_word_and_fuzzy_trigger() {
        assert!(phrase_range("hey fonography", "hey fono").is_none());
        assert!(phrase_range("hey phone", "hey fono").is_none());
        assert_eq!(
            phrase_range("well hey fono record", "hey fono"),
            Some((5, 13))
        );
    }
    #[test]
    fn needs_stability_and_distinct_observations() {
        let mut m = PhraseMatcher::new("ЭЙ ФОНО", 250, 2, 16000);
        assert_eq!(m.observe("эй фоно", 1000), None);
        assert_eq!(m.observe("эй фоно", 4999), None);
        assert_eq!(m.observe("эй фоно", 5000), Some(13));
    }
    #[test]
    fn disappearing_phrase_resets_confirmation() {
        let mut m = PhraseMatcher::new("hey fono", 100, 2, 16000);
        m.observe("hey fono", 0);
        m.observe("hey photo", 1600);
        assert_eq!(m.observe("hey fono", 3200), None);
    }
}
