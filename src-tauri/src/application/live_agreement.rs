//! LocalAgreement-2 over bounded, timestamped windows. An accepted audio
//! interval is immutable: later hypotheses can replace only the uncommitted tail.
use crate::stt::{TimedSegment, WindowTranscript};

#[derive(Default)]
pub struct LiveAgreement {
    previous: Vec<TimedSegment>,
    pub committed_end: u64,
    pub committed: String,
    pub draft: String,
    seen: Vec<TimedSegment>,
}

impl LiveAgreement {
    pub fn accept(
        &mut self,
        hypothesis: &WindowTranscript,
        audio_end: u64,
        final_window: bool,
    ) -> String {
        // Whisper token times drift. Anchor the accepted lexical suffix first;
        // every word after it is new even when its end equals the old cursor.
        let anchor = overlap_anchor(&self.seen, &hypothesis.words);
        let remaining: Vec<_> = if let Some(anchor) = anchor {
            hypothesis.words[anchor..].to_vec()
        } else {
            hypothesis
                .words
                .iter()
                .filter(|word| {
                    word.end_sample > self.committed_end
                        && !self.seen.iter().any(|seen| same_interval(word, seen))
                })
                .cloned()
                .collect()
        };
        let previous = &self.previous;
        let mut accepted = 0;
        for (index, word) in remaining.iter().enumerate() {
            if !final_window {
                let Some(old) = previous.get(index) else {
                    break;
                };
                if !same_word(&word.text, &old.text)
                    || word.end_sample > audio_end.saturating_sub(4_000)
                {
                    break;
                }
            }
            accepted += 1;
        }
        let new_text = join_words(remaining[..accepted].iter().map(|word| word.text.as_str()));
        if let Some(last) = remaining
            .get(accepted.saturating_sub(1))
            .filter(|_| accepted > 0)
        {
            self.committed_end = self.committed_end.max(last.end_sample);
            self.seen.extend_from_slice(&remaining[..accepted]);
            if self.seen.len() > 32 {
                self.seen.drain(..self.seen.len() - 32);
            }
        }
        let appended = if new_text.is_empty() {
            String::new()
        } else {
            let separator = if self.committed.is_empty()
                || new_text.starts_with(['.', ',', '!', '?', ':', ';'])
            {
                ""
            } else {
                " "
            };
            format!("{separator}{new_text}")
        };
        self.committed.push_str(&appended);
        self.draft = join_words(remaining[accepted..].iter().map(|word| word.text.as_str()));
        self.previous = remaining[accepted..].to_vec();
        appended
    }

    pub fn reset_hypothesis(&mut self) {
        self.previous.clear();
    }
}

fn same_word(a: &str, b: &str) -> bool {
    let normalize = |text: &str| {
        text.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}

fn overlap_anchor(seen: &[TimedSegment], words: &[TimedSegment]) -> Option<usize> {
    let last = seen.last()?;
    let mut best: Option<(usize, u64, usize)> = None;
    for (index, word) in words.iter().enumerate() {
        if !same_word(&last.text, &word.text) || word.end_sample.abs_diff(last.end_sample) > 16_000
        {
            continue;
        }
        let mut length = 0;
        while length < seen.len().min(index + 1)
            && same_word(
                &seen[seen.len() - 1 - length].text,
                &words[index - length].text,
            )
        {
            length += 1;
        }
        let start_distance = word.start_sample.abs_diff(last.start_sample);
        let close_start =
            start_distance <= (last.end_sample.saturating_sub(last.start_sample) / 2).min(2_400);
        if length == 1 && !same_interval(word, last) && !close_start {
            continue;
        }
        let distance = start_distance + word.end_sample.abs_diff(last.end_sample);
        let improves = match best {
            Some((old_length, old_distance, _)) => {
                length > old_length || (length == old_length && distance < old_distance)
            }
            None => true,
        };
        if improves {
            best = Some((length, distance, index + 1));
        }
    }
    best.map(|(_, _, index)| index)
}

fn same_interval(a: &TimedSegment, b: &TimedSegment) -> bool {
    let overlap = a
        .end_sample
        .min(b.end_sample)
        .saturating_sub(a.start_sample.max(b.start_sample));
    let shorter = (a.end_sample.saturating_sub(a.start_sample))
        .min(b.end_sample.saturating_sub(b.start_sample));
    shorter > 0 && overlap * 2 >= shorter
}

fn join_words<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for word in words {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        if !text.is_empty() && !word.starts_with(['.', ',', '!', '?', ':', ';']) {
            text.push(' ');
        }
        text.push_str(word);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use fono_stt_protocol::BackendKind;
    fn hypothesis(words: &[(&str, u64, u64)]) -> WindowTranscript {
        WindowTranscript {
            text: String::new(),
            words: words
                .iter()
                .map(|&(text, start_sample, end_sample)| TimedSegment {
                    text: text.into(),
                    start_sample,
                    end_sample,
                })
                .collect(),
            segments: Vec::new(),
            detected_language: None,
            transcribe_secs: 0.1,
            audio_secs: 1.0,
            backend: BackendKind::Cpu,
        }
    }
    #[test]
    fn stable_prefix_retains_repeated_words_and_unicode() {
        let mut agreement = LiveAgreement::default();
        let first = hypothesis(&[("да", 0, 2000), ("да", 2000, 4000), ("мир", 4000, 8000)]);
        assert_eq!(agreement.accept(&first, 10000, false), "");
        let second = hypothesis(&[("да", 0, 2000), ("да", 2000, 4000), ("всем", 4000, 8000)]);
        assert_eq!(agreement.accept(&second, 10000, false), "да да");
        assert_eq!(agreement.draft, "всем");
        assert_eq!(agreement.accept(&second, 10000, true), " всем");
        assert_eq!(agreement.committed, "да да всем");
    }
    #[test]
    fn final_tail_never_replaces_committed_text() {
        let mut agreement = LiveAgreement::default();
        let first = hypothesis(&[("Привет", 0, 3000), ("мир", 3000, 8000)]);
        agreement.accept(&first, 12000, false);
        assert_eq!(agreement.accept(&first, 12000, false), "Привет мир");
        let changed = hypothesis(&[
            ("Другой", 0, 3000),
            ("текст", 3000, 8000),
            ("снова", 9000, 12000),
        ]);
        assert_eq!(agreement.accept(&changed, 16000, true), " снова");
        assert_eq!(agreement.committed, "Привет мир снова");
    }
    #[test]
    fn overlapping_final_windows_do_not_duplicate_words() {
        let mut agreement = LiveAgreement::default();
        let first = hypothesis(&[("слово", 0, 3000), ("второе", 3000, 6000)]);
        agreement.accept(&first, 12000, true);
        let overlap = hypothesis(&[("второе", 3200, 6400), ("третье", 6400, 9000)]);
        assert_eq!(agreement.accept(&overlap, 12000, true), " третье");
    }
    #[test]
    fn a_real_repeat_after_the_committed_boundary_is_retained() {
        let mut agreement = LiveAgreement::default();
        agreement.accept(&hypothesis(&[("да", 0, 3000)]), 5000, true);
        assert_eq!(
            agreement.accept(&hypothesis(&[("да", 2800, 6000)]), 9000, true),
            " да"
        );
    }
    #[test]
    fn a_new_word_with_the_accepted_timestamp_is_not_lost() {
        let mut agreement = LiveAgreement::default();
        agreement.accept(&hypothesis(&[("ask", 52640, 68480)]), 96000, true);
        let next = hypothesis(&[
            ("ask", 52640, 68480),
            ("not", 64160, 68480),
            ("what", 68480, 80000),
        ]);
        agreement.accept(&next, 112000, false);
        assert_eq!(agreement.accept(&next, 128000, false), " not what");
    }
    #[test]
    fn shifted_words_are_anchored_as_an_ordered_suffix() {
        let mut agreement = LiveAgreement::default();
        agreement.accept(
            &hypothesis(&[("can", 95520, 100000), ("do", 100000, 102720)]),
            128000,
            true,
        );
        let next = hypothesis(&[
            ("can", 100800, 106080),
            ("do", 106080, 109760),
            ("for", 109760, 112000),
        ]);
        assert_eq!(agreement.accept(&next, 160000, true), " for");
    }
    #[test]
    fn a_revised_accepted_word_cannot_be_appended_again() {
        let mut agreement = LiveAgreement::default();
        agreement.accept(&hypothesis(&[("слово", 0, 3000)]), 5000, true);
        assert_eq!(
            agreement.accept(&hypothesis(&[("слова", 0, 3200)]), 7000, true),
            ""
        );
    }
}
