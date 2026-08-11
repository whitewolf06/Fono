//! Serialized repository for dictation history and its retention policy.

use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{error::AppResult, state, types::DictationHistoryEntry};

const MAX_HISTORY_ENTRIES: usize = 200;
static NEXT_HISTORY_ID: AtomicU64 = AtomicU64::new(0);
static HISTORY: Lazy<HistoryRepository> = Lazy::new(HistoryRepository::default);

#[derive(Default)]
pub struct HistoryRepository {
    write_lock: Mutex<()>,
}

impl HistoryRepository {
    pub fn list(&self) -> AppResult<Vec<DictationHistoryEntry>> {
        let _guard = self.write_lock.lock();
        state::load_history_document()
    }

    pub fn append(&self, entry: DictationHistoryEntry) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
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
        entries.retain(|entry| entry.id != id);
        state::save_history_document(&entries)
    }
}

pub fn list() -> AppResult<Vec<DictationHistoryEntry>> {
    HISTORY.list()
}

pub fn append(entry: DictationHistoryEntry) -> AppResult<()> {
    HISTORY.append(entry)
}

pub fn clear() -> AppResult<()> {
    HISTORY.clear()
}

pub fn delete(id: &str) -> AppResult<()> {
    HISTORY.delete(id)
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

    use super::next_id;

    #[test]
    fn generated_ids_do_not_collide_within_a_process() {
        let ids: HashSet<_> = (0..1_000).map(|_| next_id()).collect();
        assert_eq!(ids.len(), 1_000);
    }
}
