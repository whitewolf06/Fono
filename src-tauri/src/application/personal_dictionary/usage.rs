//! Recent/frequent canonical hints from accepted dictations in this process.
//! No transcript, usage count or timestamp is persisted, logged or sent to IPC.
use super::{normalize, replacements, validate};
use crate::types::Settings;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Priority {
    dictations: u64,
    last_used: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UsageSnapshot {
    epoch: u64,
    priorities: BTreeMap<String, Priority>,
}

impl UsageSnapshot {
    pub(super) fn priority(&self, canonical: &str) -> Priority {
        self.priorities
            .get(&normalize(canonical))
            .copied()
            .unwrap_or_default()
    }
}

pub(crate) struct UsageTracker {
    epoch: u64,
    sequence: u64,
    priorities: BTreeMap<String, Priority>,
}

impl Default for UsageTracker {
    fn default() -> Self {
        Self {
            epoch: 1,
            sequence: 0,
            priorities: BTreeMap::new(),
        }
    }
}

impl UsageTracker {
    pub(crate) fn snapshot(&self, settings: &Settings) -> UsageSnapshot {
        if !settings.personal_dictionary_enabled {
            return UsageSnapshot::default();
        }
        UsageSnapshot {
            epoch: self.epoch,
            priorities: self.priorities.clone(),
        }
    }

    pub(crate) fn settings_changed(&mut self, before: &Settings, after: &Settings) {
        let same_entries = before.personal_dictionary_entries.len()
            == after.personal_dictionary_entries.len()
            && before
                .personal_dictionary_entries
                .iter()
                .zip(&after.personal_dictionary_entries)
                .all(|(a, b)| a.written == b.written && a.spoken == b.spoken);
        if before.personal_dictionary_enabled != after.personal_dictionary_enabled || !same_entries
        {
            // Invalidates work captured before disable/re-enable or erasure.
            self.epoch = self
                .epoch
                .checked_add(1)
                .expect("dictionary epoch exhausted");
        }
        if !after.personal_dictionary_enabled
            || validate(&after.personal_dictionary_entries).is_err()
        {
            self.priorities.clear();
            self.sequence = 0;
            return;
        }
        self.priorities.retain(|canonical, _| {
            after
                .personal_dictionary_entries
                .iter()
                .any(|entry| normalize(&entry.written) == *canonical)
        });
    }

    /// Called once inside the terminal commit fence, even with history disabled.
    pub(crate) fn accepted(&mut self, current: &Settings, captured: &Settings, transcript: &str) {
        if !current.personal_dictionary_enabled
            || !captured.personal_dictionary_enabled
            || captured.personal_dictionary_usage.epoch != self.epoch
            || validate(&captured.personal_dictionary_entries).is_err()
        {
            return;
        }
        // Keep this optional bookkeeping bounded independently of recording length.
        // Skip a cut partial word rather than inventing a match at the boundary.
        let start = transcript.len().saturating_sub(16_384);
        let start = (start..=transcript.len())
            .find(|&offset| transcript.is_char_boundary(offset))
            .unwrap_or(transcript.len());
        let text = if start == 0 {
            transcript
        } else {
            let tail = &transcript[start..];
            tail.find(char::is_whitespace)
                .map(|offset| &tail[offset..])
                .unwrap_or("")
        };
        let mentioned = replacements::mentioned(text, &captured.personal_dictionary_entries);
        if mentioned.is_empty() {
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        for canonical in mentioned {
            let priority = self.priorities.entry(canonical).or_default();
            priority.dictations = priority.dictations.saturating_add(1);
            priority.last_used = self.sequence;
        }
    }
}
