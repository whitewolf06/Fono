//! SQLite persistence; callers serialize access and serve reads from their memory snapshot.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    error::{AppError, AppResult},
    types::DictationHistoryEntry,
};

const DATABASE_NAME: &str = "dictation-history.sqlite3";
const ANALYTICS_FIELDS: &[&str] = &[
    "analytics_included",
    "original_text",
    "processing",
    "analysis_status",
    "analysis",
    "analysis_error",
    "recommendation_status",
    "recommendation",
    "recommendation_error",
];

pub(super) struct HistoryStore {
    connection: Connection,
    pub directory: PathBuf,
    pub entries: Vec<DictationHistoryEntry>,
    pub revision: u64,
}

fn database_error(error: rusqlite::Error) -> AppError {
    // SQLite diagnostics are technical details, never arbitrary IPC error parameters.
    AppError::Io(std::io::Error::other(error))
}

fn connect(directory: &Path) -> AppResult<Connection> {
    std::fs::create_dir_all(directory)?;
    let connection = Connection::open(directory.join(DATABASE_NAME)).map_err(database_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(database_error)?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    if version > 1 {
        return Err(AppError::Config(
            "Unsupported history database schema".into(),
        ));
    }
    // DELETE journals avoid retaining a second archive in a WAL. secure_delete scrubs
    // freed cells for explicit deletion/retention rather than leaving recoverable text.
    connection.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA journal_mode=DELETE;
         PRAGMA synchronous=FULL;
         CREATE TABLE IF NOT EXISTS history (
             id TEXT PRIMARY KEY NOT NULL, created_at INTEGER NOT NULL, entry_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS history_created_at ON history(created_at);
         CREATE TABLE IF NOT EXISTS analyses (
             entry_id TEXT PRIMARY KEY NOT NULL REFERENCES history(id) ON DELETE CASCADE,
             payload_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS trainer_cache (id INTEGER PRIMARY KEY CHECK(id=1), payload_json TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS history_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         PRAGMA user_version=1;"
    ).map_err(database_error)?;
    Ok(connection)
}

impl HistoryStore {
    pub fn open(directory: PathBuf) -> AppResult<Self> {
        let mut connection = connect(&directory)?;
        migrate_legacy(&mut connection, &directory)?;
        let entries = load_entries(&connection)?;
        Ok(Self {
            connection,
            directory,
            entries,
            revision: 0,
        })
    }

    pub fn persist(&mut self, entries: &[DictationHistoryEntry], private: bool) -> AppResult<()> {
        let old = self
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        let new = entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        if new.len() != entries.len() {
            return Err(AppError::Config("Duplicate dictation history ID".into()));
        }
        let mut changed = Vec::new();
        for entry in entries.iter().rev() {
            let encoded = split_entry(entry)?;
            let previous = old
                .get(entry.id.as_str())
                .map(|entry| split_entry(entry))
                .transpose()?;
            if previous.as_ref() != Some(&encoded) {
                changed.push((entry, encoded, previous));
            }
        }
        if changed.is_empty() && old.len() == new.len() {
            if private {
                purge_legacy(&self.directory)?;
            }
            return Ok(());
        }
        let mut removed = false;
        let transaction = self.connection.transaction().map_err(database_error)?;
        for id in old.keys().filter(|id| !new.contains_key(**id)) {
            transaction
                .execute("DELETE FROM history WHERE id=?1", [id])
                .map_err(database_error)?;
            removed = true;
        }
        // Iterate oldest first so rowid is a stable newest-first tiebreaker on reload.
        for (entry, encoded, previous) in changed {
            upsert_entry(&transaction, entry, &encoded, previous.as_ref())?;
        }
        // A failure to remove the old JSON copies must fail the privacy mutation;
        // the SQLite transaction then rolls back, and the UI can retry safely.
        if private || removed {
            purge_legacy(&self.directory)?;
        }
        transaction.commit().map_err(database_error)?;
        self.entries = entries.to_vec();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}

fn split_entry(entry: &DictationHistoryEntry) -> AppResult<(String, String)> {
    let mut base = serde_json::to_value(entry)?;
    let object = base
        .as_object_mut()
        .ok_or_else(|| AppError::Internal("History entry must be an object".into()))?;
    let mut analytics = serde_json::Map::new();
    for key in ANALYTICS_FIELDS {
        if let Some(value) = object.remove(*key) {
            analytics.insert((*key).to_owned(), value);
        }
    }
    Ok((
        serde_json::to_string(&base)?,
        serde_json::to_string(&analytics)?,
    ))
}

fn upsert_entry(
    connection: &Connection,
    entry: &DictationHistoryEntry,
    encoded: &(String, String),
    previous: Option<&(String, String)>,
) -> AppResult<()> {
    if previous.map_or(true, |old| old.0 != encoded.0) {
        connection.execute(
        "INSERT INTO history(id,created_at,entry_json) VALUES(?1,?2,?3)
         ON CONFLICT(id) DO UPDATE SET created_at=excluded.created_at,entry_json=excluded.entry_json",
        params![entry.id, entry.created_at.timestamp_micros(), encoded.0],
    ).map_err(database_error)?;
    }
    if previous.map_or(true, |old| old.1 != encoded.1) {
        connection
            .execute(
                "INSERT INTO analyses(entry_id,payload_json) VALUES(?1,?2)
         ON CONFLICT(entry_id) DO UPDATE SET payload_json=excluded.payload_json",
                params![entry.id, encoded.1],
            )
            .map_err(database_error)?;
    }
    Ok(())
}

fn load_entries(connection: &Connection) -> AppResult<Vec<DictationHistoryEntry>> {
    let mut statement = connection.prepare(
        "SELECT h.entry_json,a.payload_json FROM history h LEFT JOIN analyses a ON a.entry_id=h.id
         ORDER BY h.created_at DESC,h.rowid DESC"
    ).map_err(database_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(database_error)?;
    rows.map(|row| {
        let (base, analysis) = row.map_err(database_error)?;
        let mut entry: serde_json::Value = serde_json::from_str(&base)?;
        if let Some(analysis) = analysis {
            let analytics: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&analysis)?;
            entry
                .as_object_mut()
                .ok_or_else(|| AppError::Internal("Invalid history database entry".into()))?
                .extend(analytics);
        }
        Ok(serde_json::from_value(entry)?)
    })
    .collect()
}

fn migrate_legacy(connection: &mut Connection, directory: &Path) -> AppResult<()> {
    let migrated: Option<String> = connection
        .query_row(
            "SELECT value FROM history_metadata WHERE key='json_migrated'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_error)?;
    if migrated.is_some() {
        return Ok(());
    }
    let legacy_path = directory.join("dictation-history.json");
    let (entries, legacy_bytes): (Vec<DictationHistoryEntry>, _) =
        crate::state::load_legacy_history_at(&legacy_path)?;
    if entries
        .iter()
        .map(|entry| &entry.id)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != entries.len()
    {
        return Err(AppError::Config(
            "Duplicate IDs in legacy history archive".into(),
        ));
    }
    if let Some(raw) = legacy_bytes {
        let backup = directory.join("dictation-history.migration.json");
        // Never overwrite the first successful migration backup on a retry.
        if !backup.exists() {
            crate::state::save_history_migration_backup(&backup, &raw)?;
        }
    }
    let transaction = connection.transaction().map_err(database_error)?;
    for entry in entries.iter().rev() {
        upsert_entry(&transaction, entry, &split_entry(entry)?, None)?;
    }
    let cache_path = directory.join("trainer-report-cache.json");
    let cache = match crate::state::load_legacy_trainer_cache_at(&cache_path) {
        Ok(cache) => cache,
        Err(_) => {
            // A derived cache must not prevent recovery of authoritative dictations.
            crate::state::delete_legacy_document(&cache_path)?;
            tracing::warn!("discarded invalid legacy trainer cache during history migration");
            None
        }
    };
    if let Some(cache) = cache {
        transaction
            .execute(
                "INSERT OR REPLACE INTO trainer_cache(id,payload_json) VALUES(1,?1)",
                [serde_json::to_string(&cache)?],
            )
            .map_err(database_error)?;
    }
    transaction
        .execute(
            "INSERT INTO history_metadata(key,value) VALUES('json_migrated','1')",
            [],
        )
        .map_err(database_error)?;
    transaction.commit().map_err(database_error)?;
    Ok(())
}

fn purge_legacy(directory: &Path) -> AppResult<()> {
    for name in [
        "dictation-history.json",
        "dictation-history.migration.json",
        "trainer-report-cache.json",
    ] {
        crate::state::delete_legacy_document(&directory.join(name))?;
    }
    Ok(())
}

pub(crate) fn load_trainer_cache<T: DeserializeOwned>(directory: &Path) -> AppResult<Option<T>> {
    let connection = connect(directory)?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT payload_json FROM trainer_cache WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_error)?;
    raw.map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

pub(crate) fn save_trainer_cache<T: Serialize>(directory: &Path, value: &T) -> AppResult<()> {
    let connection = connect(directory)?;
    connection.execute("INSERT INTO trainer_cache(id,payload_json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload_json=excluded.payload_json", [serde_json::to_string(value)?]).map_err(database_error)?;
    // Once SQLite owns the cache, a JSON recovery copy must not retain old fragments.
    crate::state::delete_legacy_document(&directory.join("trainer-report-cache.json"))
}

pub(crate) fn purge_trainer_cache(directory: &Path) -> AppResult<()> {
    let connection = connect(directory)?;
    connection
        .execute("DELETE FROM trainer_cache", [])
        .map_err(database_error)?;
    crate::state::delete_legacy_document(&directory.join("trainer-report-cache.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("fono-history-db-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn entry(id: &str) -> DictationHistoryEntry {
        serde_json::from_value(serde_json::json!({"id":id,"text":"final","created_at":"2026-10-17T09:00:00Z","device":null,"original_text":"private transcript"})).unwrap()
    }

    #[test]
    fn migration_preserves_backup_roundtrips_analytics_and_never_reimports_json() {
        for versioned in [false, true] {
            let directory = Directory::new();
            let entries = vec![entry("one"), entry("two")];
            let raw = if versioned {
                serde_json::json!({"schema_version":1,"data":entries})
            } else {
                serde_json::to_value(&entries).unwrap()
            };
            let bytes = serde_json::to_vec_pretty(&raw).unwrap();
            std::fs::write(directory.0.join("dictation-history.json"), &bytes).unwrap();
            std::fs::write(
                directory.0.join("trainer-report-cache.json"),
                br#"{"cache":"private report"}"#,
            )
            .unwrap();
            let store = HistoryStore::open(directory.0.clone()).unwrap();
            assert_eq!(store.entries.len(), 2);
            assert_eq!(
                store.entries[0].original_text.as_deref(),
                Some("private transcript")
            );
            assert_eq!(
                std::fs::read(directory.0.join("dictation-history.migration.json")).unwrap(),
                bytes
            );
            let cache: serde_json::Value = load_trainer_cache(&directory.0).unwrap().unwrap();
            assert_eq!(cache["cache"], "private report");
            drop(store);
            std::fs::write(directory.0.join("dictation-history.json"), b"broken JSON").unwrap();
            assert_eq!(
                HistoryStore::open(directory.0.clone())
                    .unwrap()
                    .entries
                    .len(),
                2
            );
        }
    }

    #[test]
    fn migration_recovers_legacy_backup_without_losing_source() {
        let directory = Directory::new();
        let backup = serde_json::to_vec(&vec![entry("backup")]).unwrap();
        std::fs::write(directory.0.join("dictation-history.json"), b"broken").unwrap();
        std::fs::write(directory.0.join("dictation-history.json.bak"), &backup).unwrap();
        let store = HistoryStore::open(directory.0.clone()).unwrap();
        assert_eq!(store.entries[0].id, "backup");
        assert_eq!(
            std::fs::read(directory.0.join("dictation-history.json.bak")).unwrap(),
            backup
        );
        let migrated_backup =
            std::fs::read(directory.0.join("dictation-history.migration.json")).unwrap();
        assert_eq!(migrated_backup, backup);
        let recovered: Vec<DictationHistoryEntry> =
            serde_json::from_slice(&migrated_backup).unwrap();
        assert_eq!(recovered[0].id, "backup");
        // Migration no longer needs to repair or overwrite the corrupt primary.
        assert_eq!(
            std::fs::read(directory.0.join("dictation-history.json")).unwrap(),
            b"broken"
        );
    }

    #[test]
    fn failed_migration_does_not_mark_source_as_imported() {
        let directory = Directory::new();
        std::fs::write(
            directory.0.join("dictation-history.json"),
            b"{\"schema_version\":99,\"data\":[]}",
        )
        .unwrap();
        assert!(HistoryStore::open(directory.0.clone()).is_err());
        std::fs::write(
            directory.0.join("dictation-history.json"),
            serde_json::to_vec(&vec![entry("retry")]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            HistoryStore::open(directory.0.clone()).unwrap().entries[0].id,
            "retry"
        );
    }

    #[test]
    fn invalid_derived_cache_does_not_block_authoritative_history_migration() {
        let directory = Directory::new();
        std::fs::write(
            directory.0.join("dictation-history.json"),
            serde_json::to_vec(&vec![entry("safe")]).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.0.join("trainer-report-cache.json"),
            b"broken private cache",
        )
        .unwrap();
        let store = HistoryStore::open(directory.0.clone()).unwrap();
        assert_eq!(store.entries[0].id, "safe");
        assert!(directory
            .0
            .join("dictation-history.migration.json")
            .exists());
        assert!(!directory.0.join("trainer-report-cache.json").exists());
        assert!(load_trainer_cache::<serde_json::Value>(&directory.0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn incremental_mutations_are_atomic_and_failed_writes_leave_memory_unchanged() {
        let directory = Directory::new();
        let mut store = HistoryStore::open(directory.0.clone()).unwrap();
        store.persist(&[entry("one"), entry("two")], false).unwrap();
        let before = store.connection.total_changes();
        let mut changed = store.entries.clone();
        changed[0].text = "changed".into();
        store.persist(&changed, false).unwrap();
        assert_eq!(
            store.connection.total_changes() - before,
            1,
            "only the changed history row is written"
        );
        let before = store.connection.total_changes();
        store.persist(&changed, false).unwrap();
        assert_eq!(
            store.connection.total_changes(),
            before,
            "unchanged maintenance does not write"
        );
        store.connection.execute_batch("CREATE TRIGGER fail_entry BEFORE INSERT ON history WHEN NEW.id='fail' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        changed.push(entry("fail"));
        assert!(store.persist(&changed, false).is_err());
        assert_eq!(store.entries.len(), 2);
        assert_eq!(load_entries(&store.connection).unwrap().len(), 2);
        assert_eq!(store.entries[0].text, "changed");
    }

    #[test]
    fn deletion_cascades_analytics_and_purges_all_json_recovery_copies() {
        let directory = Directory::new();
        let raw = serde_json::to_vec(&vec![entry("private")]).unwrap();
        for name in ["dictation-history.json", "dictation-history.json.bak"] {
            std::fs::write(directory.0.join(name), &raw).unwrap();
        }
        let mut store = HistoryStore::open(directory.0.clone()).unwrap();
        store.persist(&[], true).unwrap();
        for name in [
            "dictation-history.json",
            "dictation-history.json.bak",
            "dictation-history.migration.json",
        ] {
            assert!(!directory.0.join(name).exists());
        }
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM analyses", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
        drop(store);
        let database = std::fs::read(directory.0.join(DATABASE_NAME)).unwrap();
        assert!(!database
            .windows(b"private transcript".len())
            .any(|window| window == b"private transcript"));
        assert!(HistoryStore::open(directory.0.clone())
            .unwrap()
            .entries
            .is_empty());
    }
}
