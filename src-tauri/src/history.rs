//! Serialized repository for dictation history and its retention policy.

use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{
    error::AppResult,
    state,
    types::{DictationAnalysisStatus, DictationHistoryEntry},
};

const MAX_HISTORY_ENTRIES: usize = 200;
static NEXT_HISTORY_ID: AtomicU64 = AtomicU64::new(0);
static HISTORY: Lazy<HistoryRepository> = Lazy::new(HistoryRepository::default);

#[derive(Default)]
pub struct HistoryRepository {
    write_lock: Mutex<()>,
}

impl HistoryRepository {
    pub fn list(
        &self,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<Vec<DictationHistoryEntry>> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        ) {
            state::save_history_document(&entries)?;
        }
        Ok(entries)
    }

    pub fn append(
        &self,
        entry: DictationHistoryEntry,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        );
        entries.insert(0, entry);
        entries.truncate(MAX_HISTORY_ENTRIES);
        state::save_history_document(&entries)
    }

    pub fn clear(&self) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        state::save_history_document::<DictationHistoryEntry>(&[])
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        remove_entry(&mut entries, id);
        state::save_history_document(&entries)
    }

    pub fn apply_analytics_privacy_policy(
        &self,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        ) {
            state::save_history_document(&entries)?;
        }
        Ok(())
    }
}

pub fn list(
    analytics_enabled: bool,
    analytics_retention_days: u16,
) -> AppResult<Vec<DictationHistoryEntry>> {
    HISTORY.list(analytics_enabled, analytics_retention_days)
}

pub fn append(
    entry: DictationHistoryEntry,
    analytics_enabled: bool,
    analytics_retention_days: u16,
) -> AppResult<()> {
    HISTORY.append(entry, analytics_enabled, analytics_retention_days)
}

pub fn clear() -> AppResult<()> {
    HISTORY.clear()
}

pub fn delete(id: &str) -> AppResult<()> {
    HISTORY.delete(id)
}

pub fn apply_analytics_privacy_policy(
    analytics_enabled: bool,
    analytics_retention_days: u16,
) -> AppResult<()> {
    HISTORY.apply_analytics_privacy_policy(analytics_enabled, analytics_retention_days)
}

fn apply_analytics_privacy_policy_to_entries(
    entries: &mut [DictationHistoryEntry],
    analytics_enabled: bool,
    analytics_retention_days: u16,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    entries.iter_mut().fold(false, |changed, entry| {
        let status = if !analytics_enabled {
            Some(DictationAnalysisStatus::Disabled)
        } else if now.signed_duration_since(entry.created_at).num_days()
            >= i64::from(analytics_retention_days)
        {
            Some(DictationAnalysisStatus::Expired)
        } else {
            None
        };

        let entry_changed =
            status.is_some_and(|next_status| entry.clear_analytics_data(next_status));
        changed || entry_changed
    })
}

fn remove_entry(entries: &mut Vec<DictationHistoryEntry>, id: &str) -> bool {
    let previous_len = entries.len();
    entries.retain(|entry| entry.id != id);
    entries.len() != previous_len
}

pub fn next_id() -> String {
    let sequence = NEXT_HISTORY_ID.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{sequence}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::{Duration, Utc};

    use crate::types::{
        AiMode, DictationAnalysisStatus, DictationHistoryEntry, DictationProcessingMetadata,
    };

    use super::{apply_analytics_privacy_policy_to_entries, next_id, remove_entry};

    #[test]
    fn generated_ids_do_not_collide_within_a_process() {
        let ids: HashSet<_> = (0..1_000).map(|_| next_id()).collect();
        assert_eq!(ids.len(), 1_000);
    }

    #[test]
    fn disabled_analytics_removes_original_text_and_metadata() {
        let mut entries = vec![entry_with_analytics(Utc::now())];

        assert!(apply_analytics_privacy_policy_to_entries(
            &mut entries,
            false,
            30,
            Utc::now()
        ));
        assert_eq!(
            entries[0].analysis_status,
            DictationAnalysisStatus::Disabled
        );
        assert!(entries[0].original_text.is_none());
        assert!(entries[0].processing.is_none());
        assert_eq!(entries[0].text, "final text");
    }

    #[test]
    fn retention_expires_analytics_without_deleting_history_result() {
        let now = Utc::now();
        let mut entries = vec![entry_with_analytics(now - Duration::days(30))];

        assert!(apply_analytics_privacy_policy_to_entries(
            &mut entries,
            true,
            30,
            now
        ));
        assert_eq!(entries[0].analysis_status, DictationAnalysisStatus::Expired);
        assert!(entries[0].original_text.is_none());
        assert_eq!(entries[0].text, "final text");
    }

    #[test]
    fn deleting_history_entry_removes_its_analytics_payload_with_it() {
        let mut entries = vec![entry_with_analytics(Utc::now())];

        assert!(remove_entry(&mut entries, "entry"));
        assert!(entries.is_empty());
    }

    fn entry_with_analytics(created_at: chrono::DateTime<chrono::Utc>) -> DictationHistoryEntry {
        DictationHistoryEntry {
            id: "entry".into(),
            text: "final text".into(),
            created_at,
            device: Some("CUDA".into()),
            original_text: Some("original text".into()),
            processing: Some(DictationProcessingMetadata {
                ai_mode: AiMode::Clean,
                detected_language: Some("ru".into()),
                transcribe_secs: Some(1.0),
                audio_secs: Some(2.0),
            }),
            analysis_status: DictationAnalysisStatus::Pending,
        }
    }
}
