use super::*;
use crate::types::Settings;
use std::sync::Arc;

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-rewrite-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn repository(&self) -> Arc<HistoryRepository> {
        Arc::new(HistoryRepository {
            directory: Some(self.0.clone()),
            ..HistoryRepository::default()
        })
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup() -> (Directory, Arc<HistoryRepository>) {
    let directory = Directory::new();
    let repository = directory.repository();
    let settings = Settings {
        analytics_enabled: true,
        history_enabled: true,
        ..Settings::default()
    };
    repository.maintain(&settings).unwrap();
    let entry: DictationHistoryEntry = serde_json::from_value(serde_json::json!({"id":"entry","text":"final session text","original_text":"original transcript","created_at":chrono::Utc::now(),"device":null})).unwrap();
    repository.append(entry, true, 30).unwrap();
    (directory, repository)
}
fn variant(repository: &HistoryRepository, source: RewriteSource) -> (RewriteVariant, u64) {
    let source_material = repository.rewrite_material("entry", source).unwrap();
    (
        RewriteVariant {
            id: cache_key(
                "entry",
                &source_material.fingerprint,
                RewriteStyle::Preserve,
                "profile-revision",
            )
            .unwrap(),
            entry_id: "entry".into(),
            source,
            source_fingerprint: source_material.fingerprint,
            style: RewriteStyle::Preserve,
            profile_id: "p".into(),
            profile_name: "profile".into(),
            profile_revision: "profile-revision".into(),
            model: "model".into(),
            created_at: chrono::Utc::now(),
            status: RewriteStatus::Ready,
            text: "proposed text".into(),
            stale: false,
        },
        source_material.epoch,
    )
}

#[test]
fn persisted_variant_is_deduplicated_without_mutating_original_or_result() {
    let (directory, repository) = setup();
    let before = serde_json::to_value(repository.list(true, 30).unwrap()).unwrap();
    let (mut value, epoch) = variant(&repository, RewriteSource::Original);
    repository.save_rewrite(&value, epoch).unwrap();
    value.text = "duplicate request must not replace saved result".into();
    assert_eq!(
        repository.save_rewrite(&value, epoch).unwrap().text,
        "proposed text"
    );
    assert_eq!(repository.rewrite_variants("entry").unwrap().len(), 1);
    assert_eq!(
        serde_json::to_value(repository.list(true, 30).unwrap()).unwrap(),
        before
    );
    drop(repository);
    let reopened = directory.repository();
    assert_eq!(
        reopened.rewrite_variants("entry").unwrap()[0].text,
        "proposed text"
    );
    assert_eq!(
        serde_json::to_value(reopened.list(true, 30).unwrap()).unwrap(),
        before
    );
}

#[test]
fn deletion_and_clear_cascade_and_reject_late_completion() {
    for clear in [false, true] {
        let (directory, repository) = setup();
        let (value, epoch) = variant(&repository, RewriteSource::Result);
        repository.save_rewrite(&value, epoch).unwrap();
        if clear {
            repository.clear().unwrap();
        } else {
            repository.delete("entry").unwrap();
        }
        assert!(repository.save_rewrite(&value, epoch).is_err());
        drop(repository);
        let connection =
            rusqlite::Connection::open(directory.0.join("dictation-history.sqlite3")).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM history_rewrites", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn analytics_revocation_removes_original_derived_text_and_fences_inflight_work() {
    let (_directory, repository) = setup();
    let (original, epoch) = variant(&repository, RewriteSource::Original);
    let (result, _) = variant(&repository, RewriteSource::Result);
    repository.save_rewrite(&original, epoch).unwrap();
    repository.save_rewrite(&result, epoch).unwrap();
    repository.clear_analytics().unwrap();
    let remaining = repository.rewrite_variants("entry").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].source, RewriteSource::Result);
    assert!(repository.save_rewrite(&original, epoch).is_err());
    assert!(repository.save_rewrite(&result, epoch).is_err());
}

#[test]
fn delete_variant_invalidates_pending_completion_and_does_not_delete_history() {
    let (_directory, repository) = setup();
    let (value, epoch) = variant(&repository, RewriteSource::Result);
    repository.save_rewrite(&value, epoch).unwrap();
    repository.delete_rewrite("other-entry", &value.id).unwrap();
    assert_eq!(repository.rewrite_variants("entry").unwrap().len(), 1);
    repository.delete_rewrite("entry", &value.id).unwrap();
    assert!(repository.rewrite_variants("entry").unwrap().is_empty());
    assert!(repository.save_rewrite(&value, epoch).is_err());
    assert_eq!(repository.list(true, 30).unwrap().len(), 1);
}

#[test]
fn schema_one_upgrades_without_inventing_variants_or_delivery_facts() {
    let (directory, repository) = setup();
    drop(repository);
    let connection =
        rusqlite::Connection::open(directory.0.join("dictation-history.sqlite3")).unwrap();
    connection
        .execute_batch("DROP TABLE history_rewrites; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    let repository = directory.repository();
    assert_eq!(repository.list(true, 30).unwrap().len(), 1);
    let context =
        crate::application::history_rewrite::context(&repository, &Settings::default(), "entry")
            .unwrap();
    assert_eq!(context.external_delivery, "unknown");
    assert!(context.variants.is_empty());
}

#[test]
fn profile_or_source_mismatch_is_stale_and_missing_original_is_not_fabricated() {
    let (_directory, repository) = setup();
    let (value, epoch) = variant(&repository, RewriteSource::Result);
    repository.save_rewrite(&value, epoch).unwrap();
    let context =
        crate::application::history_rewrite::context(&repository, &Settings::default(), "entry")
            .unwrap();
    assert!(context.variants[0].stale);
    repository.clear_analytics().unwrap();
    assert!(repository
        .rewrite_material("entry", RewriteSource::Original)
        .is_err());
}

#[test]
fn cancellation_at_storage_boundary_rolls_back_and_styles_have_distinct_cache_keys() {
    let (_directory, repository) = setup();
    let (value, epoch) = variant(&repository, RewriteSource::Result);
    assert!(repository
        .save_rewrite_guarded(&value, epoch, || false)
        .is_err());
    assert!(repository.rewrite_variants("entry").unwrap().is_empty());
    let keys: std::collections::BTreeSet<_> = [
        RewriteStyle::Preserve,
        RewriteStyle::Concise,
        RewriteStyle::Restrained,
        RewriteStyle::Friendly,
        RewriteStyle::Lively,
    ]
    .into_iter()
    .map(|style| {
        cache_key(
            "entry",
            &value.source_fingerprint,
            style,
            "profile-revision",
        )
        .unwrap()
    })
    .collect();
    assert_eq!(keys.len(), 5);
}

#[test]
fn explicit_variants_follow_history_retention_without_a_silent_count_limit() {
    let (directory, repository) = setup();
    let (base, epoch) = variant(&repository, RewriteSource::Result);
    for index in 0..25 {
        let mut value = base.clone();
        value.profile_revision = format!("revision-{index}");
        value.id = cache_key(
            "entry",
            &value.source_fingerprint,
            value.style,
            &value.profile_revision,
        )
        .unwrap();
        repository.save_rewrite(&value, epoch).unwrap();
    }
    assert_eq!(repository.rewrite_variants("entry").unwrap().len(), 25);
    drop(repository);
    let reopened = directory.repository();
    assert_eq!(reopened.rewrite_variants("entry").unwrap().len(), 25);
    reopened.delete("entry").unwrap();
    assert!(reopened.rewrite_variants("entry").is_err());
}
