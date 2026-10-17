use super::*;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct StoredValue {
    value: String,
}

fn temporary_test_path(name: &str) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("fono-state-test-{}-{sequence}", std::process::id()));
    fs::create_dir_all(&directory).expect("create isolated test directory");
    directory.join(name)
}

#[test]
fn atomic_write_keeps_previous_complete_version_as_backup() {
    let path = temporary_test_path("settings.json");
    let first = StoredValue {
        value: "first".into(),
    };
    let second = StoredValue {
        value: "second".into(),
    };

    save_json_atomically(&path, &first).expect("write first version");
    save_json_atomically(&path, &second).expect("replace with second version");

    let current: StoredValue =
        serde_json::from_str(&fs::read_to_string(&path).expect("read current version"))
            .expect("deserialize current version");
    let backup: StoredValue =
        serde_json::from_str(&fs::read_to_string(backup_path(&path)).expect("read backup version"))
            .expect("deserialize backup version");
    assert_eq!(current, second);
    assert_eq!(backup, first);

    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn corrupted_primary_json_recovers_last_valid_backup() {
    let path = temporary_test_path("history.json");
    let previous = StoredValue {
        value: "previous".into(),
    };
    let current = StoredValue {
        value: "current".into(),
    };

    save_json_atomically(&path, &previous).expect("write previous version");
    save_json_atomically(&path, &current).expect("write current version");
    fs::write(&path, "{ invalid json").expect("corrupt primary JSON");

    let recovered = load_json_with_backup::<StoredValue>(&path)
        .expect("recover backup")
        .expect("backup value exists");
    assert_eq!(recovered, previous);

    let restored: StoredValue =
        serde_json::from_str(&fs::read_to_string(&path).expect("read restored primary"))
            .expect("deserialize restored primary");
    assert_eq!(restored, previous);

    let preserved_backup: StoredValue = serde_json::from_str(
        &fs::read_to_string(backup_path(&path)).expect("read preserved backup"),
    )
    .expect("deserialize preserved backup");
    assert_eq!(preserved_backup, previous);

    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn legacy_document_is_detected_and_rewritten_with_schema_version() {
    let path = temporary_test_path("legacy.json");
    let legacy = StoredValue {
        value: "legacy".into(),
    };
    save_json_atomically(&path, &legacy).expect("write legacy document");

    let (loaded, needs_migration) = load_versioned_json_with_backup::<StoredValue>(&path)
        .expect("load legacy document")
        .expect("legacy document exists");
    assert_eq!(loaded, legacy);
    assert!(needs_migration);

    save_versioned_json_atomically(&path, &loaded).expect("write versioned document");
    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read versioned document"))
            .expect("parse versioned document");
    assert_eq!(raw["schema_version"], PERSISTENCE_SCHEMA_VERSION);
    assert_eq!(raw["data"]["value"], "legacy");

    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn future_schema_version_is_rejected() {
    let path = temporary_test_path("future.json");
    fs::write(&path, r#"{"schema_version":999,"data":{"value":"future"}}"#)
        .expect("write future document");

    let error = load_versioned_json_with_backup::<StoredValue>(&path).unwrap_err();
    assert!(error.to_string().contains("unsupported persistence schema"));

    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn private_save_sanitizes_primary_and_recovery_without_resurrecting_old_content() {
    let path = temporary_test_path("private-history.json");
    save_versioned_json_atomically(&path, &vec!["privatefixture"])
        .expect("write initial private history");
    save_versioned_json_atomically(&path, &vec!["privatefixture", "kept result"])
        .expect("make recovery backup");
    let abandoned = temporary_path(&path);
    fs::write(&abandoned, "privatefixture").expect("make an interrupted writer fixture");

    save_versioned_json_privately(&path, &vec!["kept result"]).expect("sanitize history");
    for copy in [&path, &backup_path(&path)] {
        let raw = fs::read_to_string(copy).expect("read sanitized history copy");
        assert!(!raw.contains("privatefixture"));
        assert!(raw.contains("kept result"));
    }
    assert!(!abandoned.exists());
    fs::write(&path, "malformed").expect("damage primary");
    let (recovered, _) = load_versioned_json_with_backup::<Vec<String>>(&path)
        .expect("recover sanitized archive")
        .expect("archive exists");
    assert_eq!(recovered, vec!["kept result"]);
    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn cache_purge_removes_primary_backup_and_only_its_own_abandoned_temporary() {
    let path = temporary_test_path("trainer-cache.json");
    save_json_atomically(&path, &"privatefixture").expect("write cache");
    save_json_atomically(&path, &"new privatefixture").expect("write cache backup");
    let abandoned = temporary_path(&path);
    fs::write(&abandoned, "privatefixture").expect("write abandoned cache");
    let unrelated = path.parent().unwrap().join("other.json.tmp");
    fs::write(&unrelated, "keep unrelated").expect("write unrelated file");
    delete_json_with_backup(&path).expect("purge cache copies");
    assert!(!path.exists());
    assert!(!backup_path(&path).exists());
    assert!(!abandoned.exists());
    assert!(unrelated.exists());
    assert!(load_json_with_backup::<String>(&path).unwrap().is_none());
    fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
}

#[test]
fn command_proposal_is_invalidated_when_settings_change() {
    let state = AppState::new();
    let proposal = crate::application::command_proposal::create(
        &state,
        7,
        fono_core::OperationSource::Hotkey,
        "громче".into(),
        None,
    );
    assert!(state.pending_command_proposal().is_some());

    state.set_settings(Settings::default());
    let error = state.take_pending_command_proposal().unwrap_err();
    assert!(error.to_string().contains("invalidated"));
    assert_eq!(proposal.operation_id, 7);
}

#[test]
fn expired_command_proposal_cannot_be_confirmed() {
    let state = AppState::new();
    let mut proposal = crate::application::command_proposal::create(
        &state,
        8,
        fono_core::OperationSource::Hotkey,
        "тише".into(),
        None,
    );
    proposal.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    state.set_pending_command_proposal(Some(proposal));

    assert!(state.pending_command_proposal().is_none());
    assert!(state.take_pending_command_proposal().is_err());
}
