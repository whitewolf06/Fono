use std::collections::HashSet;

use chrono::{Duration, Utc};

use crate::types::{
    AiMode, DictationAnalysisStatus, DictationHistoryEntry, DictationProcessingMetadata,
    SpeechSessionAnalysis,
};

use super::{
    apply_analytics_privacy_policy_to_entries, build_period_report, complete_analysis_in_entries,
    next_id, remove_entry,
};

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

#[test]
fn period_report_aggregates_ready_sessions_by_day() {
    let created_at = chrono::DateTime::parse_from_rfc3339("2026-08-31T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut entry = entry_with_analytics(created_at);
    entry.analysis_status = DictationAnalysisStatus::Ready;
    entry.analysis = Some(SpeechSessionAnalysis {
        word_count: 20,
        filler_count: 2,
        filler_density_per_100_words: 10.0,
        repetition_count: 1,
        self_correction_count: 1,
        unfinished_count: 0,
        findings: Vec::new(),
    });
    let from = created_at - Duration::hours(1);
    let to = created_at + Duration::hours(1);

    let report = build_period_report(vec![entry], from, to);

    assert_eq!(report.analyzed_sessions, 1);
    assert_eq!(report.total_words, 20);
    assert_eq!(report.filler_count, 2);
    assert_eq!(report.daily.len(), 1);
    assert_eq!(report.daily[0].date.to_string(), "2026-08-31");
}

#[test]
fn period_report_ignores_excluded_sessions() {
    let created_at = Utc::now();
    let mut included = entry_with_analytics(created_at);
    included.analysis_status = DictationAnalysisStatus::Ready;
    included.analysis = Some(SpeechSessionAnalysis {
        word_count: 10,
        filler_count: 1,
        filler_density_per_100_words: 10.0,
        repetition_count: 0,
        self_correction_count: 0,
        unfinished_count: 0,
        findings: Vec::new(),
    });
    let mut excluded = included.clone();
    excluded.id = "excluded".into();
    excluded.analytics_included = false;
    excluded.analysis.as_mut().unwrap().word_count = 100;
    excluded.analysis.as_mut().unwrap().filler_count = 100;

    let report = build_period_report(
        vec![included, excluded],
        created_at - Duration::hours(1),
        created_at + Duration::hours(1),
    );

    assert_eq!(report.analyzed_sessions, 1);
    assert_eq!(report.total_words, 10);
    assert_eq!(report.filler_count, 1);
}

#[test]
fn excluded_pending_session_cannot_complete_analysis() {
    let mut entries = vec![entry_with_analytics(Utc::now())];
    entries[0].analytics_included = false;

    assert!(!complete_analysis_in_entries(
        &mut entries,
        "entry",
        SpeechSessionAnalysis {
            word_count: 3,
            filler_count: 0,
            filler_density_per_100_words: 0.0,
            repetition_count: 0,
            self_correction_count: 0,
            unfinished_count: 0,
            findings: Vec::new(),
        }
    ));
    assert_eq!(entries[0].analysis_status, DictationAnalysisStatus::Pending);
}

#[test]
fn queue_completion_is_idempotent_for_one_pending_session() {
    let mut entries = vec![entry_with_analytics(Utc::now())];
    let analysis = SpeechSessionAnalysis {
        word_count: 3,
        filler_count: 0,
        filler_density_per_100_words: 0.0,
        repetition_count: 0,
        self_correction_count: 0,
        unfinished_count: 0,
        findings: Vec::new(),
    };

    assert!(complete_analysis_in_entries(
        &mut entries,
        "entry",
        analysis.clone()
    ));
    assert!(!complete_analysis_in_entries(
        &mut entries,
        "entry",
        analysis
    ));
    assert_eq!(entries[0].analysis_status, DictationAnalysisStatus::Ready);
    assert_eq!(entries[0].analysis.as_ref().unwrap().word_count, 3);
}

struct TestDirectory(std::path::PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn repository(&self) -> super::HistoryRepository {
        super::HistoryRepository {
            directory: Some(self.0.clone()),
            hooks: super::HistoryHooks {
                before_private_write: std::sync::Arc::new(
                    crate::application::trainer_report::invalidate_private_cache_at,
                ),
                ..super::HistoryHooks::default()
            },
            ..super::HistoryRepository::default()
        }
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn memory_reads_do_not_reload_sqlite_or_rewrite_privacy_data() {
    let directory = TestDirectory::new();
    let repository = directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    let connection =
        rusqlite::Connection::open(directory.0.join("dictation-history.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE history SET entry_json=json_set(entry_json,'$.text','disk-only text')",
            [],
        )
        .unwrap();
    let memory = repository.list(false, 0).unwrap();
    assert_eq!(memory[0].text, "final text");
    assert!(memory[0].original_text.is_some());
    // A separately opened repository sees the deliberate external SQL update.
    assert_eq!(
        directory.repository().list(true, 30).unwrap()[0].text,
        "disk-only text"
    );
}

#[test]
fn archive_keeps_more_than_200_sessions_and_ninety_day_report_uses_all() {
    let directory = TestDirectory::new();
    let now = Utc::now();
    let entries = (0..600)
        .map(|index| {
            let mut entry = entry_with_analytics(now - Duration::hours(index * 3));
            entry.id = format!("session-{index}");
            entry.analysis_status = DictationAnalysisStatus::Ready;
            entry.analysis = Some(crate::speech_metrics::analyze_russian_speech(
                "one two three",
            ));
            entry
        })
        .collect::<Vec<_>>();
    {
        let mut store = super::storage::HistoryStore::open(directory.0.clone()).unwrap();
        store.persist(&entries, false).unwrap();
    }
    let repository = directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        analytics_retention_days: 90,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    assert_eq!(repository.list(true, 90).unwrap().len(), 600);
    let mut extra = entry_with_analytics(now);
    extra.id = "newest".into();
    repository.append(extra, true, 90).unwrap();
    assert_eq!(repository.list(true, 90).unwrap().len(), 601);
    let report = repository
        .period_report(now - Duration::days(90), now + Duration::seconds(1))
        .unwrap();
    assert_eq!(report.analyzed_sessions, 600);
    assert_eq!(report.total_words, 1800);
}

#[test]
fn retention_and_analytics_maintenance_scrub_database_and_migration_backups() {
    let directory = TestDirectory::new();
    let now = Utc::now();
    let mut recent = entry_with_analytics(now - Duration::days(2));
    recent.id = "recent".into();
    let mut old = recent.clone();
    old.id = "old".into();
    old.created_at = now - Duration::days(101);
    let legacy = serde_json::to_vec(&vec![recent, old]).unwrap();
    std::fs::write(directory.0.join("dictation-history.json"), legacy).unwrap();
    let repository = directory.repository();
    assert_eq!(repository.list(true, 90).unwrap().len(), 2);
    assert!(directory
        .0
        .join("dictation-history.migration.json")
        .exists());
    let settings = crate::types::Settings {
        analytics_enabled: true,
        history_retention_days: 100,
        analytics_retention_days: 1,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    let entries = repository.list(true, 90).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "recent");
    assert_eq!(entries[0].text, "final text");
    assert!(entries[0].original_text.is_none());
    assert_eq!(entries[0].analysis_status, DictationAnalysisStatus::Expired);
    assert!(!directory.0.join("dictation-history.json").exists());
    assert!(!directory
        .0
        .join("dictation-history.migration.json")
        .exists());
    let connection =
        rusqlite::Connection::open(directory.0.join("dictation-history.sqlite3")).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM history", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM analyses", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn revoked_consent_cannot_be_reenabled_by_older_append_settings() {
    let directory = TestDirectory::new();
    let repository = directory.repository();
    repository
        .maintain(&crate::types::Settings::default())
        .unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 90)
        .unwrap();
    assert!(repository.list(true, 90).unwrap()[0]
        .original_text
        .is_none());
}

#[test]
fn invalid_retention_from_disk_cannot_prune_the_archive() {
    let directory = TestDirectory::new();
    let repository = directory.repository();
    repository
        .append(entry_with_analytics(Utc::now()), false, 30)
        .unwrap();
    let invalid = crate::types::Settings {
        history_retention_days: 0,
        ..crate::types::Settings::default()
    };
    assert!(repository.maintain(&invalid).is_err());
    assert!(repository.list(false, 30).is_err());
    repository
        .maintain(&crate::types::Settings::default())
        .unwrap();
    assert_eq!(repository.list(false, 30).unwrap().len(), 1);
}

#[test]
fn failed_privacy_transaction_blocks_source_reads_until_successful_retry() {
    let directory = TestDirectory::new();
    let repository = directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    let connection =
        rusqlite::Connection::open(directory.0.join("dictation-history.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_private BEFORE UPDATE ON analyses BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(repository
        .maintain(&crate::types::Settings::default())
        .is_err());
    assert!(repository.list(false, 30).is_err());
    assert!(repository.pending_analysis_input("entry").is_err());
    connection
        .execute_batch("DROP TRIGGER fail_private;")
        .unwrap();
    repository
        .maintain(&crate::types::Settings::default())
        .unwrap();
    assert!(repository.list(false, 30).unwrap()[0]
        .original_text
        .is_none());
}

#[test]
fn clearing_old_analytics_preserves_consent_for_future_dictations() {
    let directory = TestDirectory::new();
    let repository = directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    repository.clear_analytics().unwrap();
    let mut next = entry_with_analytics(Utc::now());
    next.id = "new".into();
    repository.append(next, true, 30).unwrap();
    let entries = repository.list(true, 30).unwrap();
    assert!(entries
        .iter()
        .find(|entry| entry.id == "entry")
        .unwrap()
        .original_text
        .is_none());
    assert!(entries
        .iter()
        .find(|entry| entry.id == "new")
        .unwrap()
        .original_text
        .is_some());
}

#[test]
fn observers_run_only_after_durable_mutation_and_without_repository_lock() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let directory = TestDirectory::new();
    let repository = Arc::new(directory.repository());
    repository
        .maintain(&crate::types::Settings::default())
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let weak = Arc::downgrade(&repository);
    let notified = count.clone();
    *repository.change_observer.lock() = Some(Arc::new(move |_| {
        assert!(weak.upgrade().unwrap().store.try_lock().is_some());
        notified.fetch_add(1, Ordering::SeqCst);
    }));
    repository.list(false, 30).unwrap();
    repository
        .maintain(&crate::types::Settings::default())
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    repository
        .append(entry_with_analytics(Utc::now()), false, 30)
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    repository.delete("missing").unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    repository.delete("entry").unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn privacy_hook_failure_blocks_sources_until_successful_retry() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let directory = TestDirectory::new();
    let failed = Arc::new(AtomicBool::new(false));
    let fail = failed.clone();
    let mut repository = directory.repository();
    repository.hooks.before_private_write = Arc::new(move |directory| {
        if fail.load(Ordering::SeqCst) {
            return Err(crate::error::AppError::Config(
                "privacy fixture unavailable".into(),
            ));
        }
        crate::application::trainer_report::invalidate_private_cache_at(directory)
    });
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    failed.store(true, Ordering::SeqCst);
    assert!(repository.clear_analytics().is_err());
    assert!(repository.list(true, 30).is_err());
    assert!(repository.pending_analysis_input("entry").is_err());
    failed.store(false, Ordering::SeqCst);
    repository.clear_analytics().unwrap();
    assert!(repository.list(true, 30).unwrap()[0]
        .original_text
        .is_none());
}

#[test]
fn unchanged_enabled_policy_preserves_active_work_and_analytics() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let directory = TestDirectory::new();
    let mut repository = directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    repository.maintain(&settings).unwrap();
    repository
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    let request = crate::application::trainer_ai::SessionRequest::begin(&format!(
        "history-policy-{}",
        uuid::Uuid::new_v4()
    ))
    .unwrap();
    let revoked = request.token.clone();
    repository.hooks.sources_changed = Arc::new(move |change| {
        if change.kind == super::HistoryChangeKind::Privacy {
            revoked.cancel();
        }
    });
    let attempts = Arc::new(AtomicUsize::new(0));
    let captured = attempts.clone();
    repository.hooks.before_private_write = Arc::new(move |_| {
        captured.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    repository.maintain(&settings).unwrap();
    repository.apply_analytics_privacy_policy(true, 30).unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
    assert!(!request.token.is_cancelled());
    assert_eq!(
        repository
            .pending_analysis_input("entry")
            .unwrap()
            .as_deref(),
        Some("original text")
    );
}

#[derive(Clone, Copy)]
enum PrivacyMutation {
    Delete,
    Exclude,
    Clear,
}

impl PrivacyMutation {
    fn apply(self, repository: &super::HistoryRepository) -> crate::error::AppResult<()> {
        match self {
            Self::Delete => repository.delete("entry"),
            Self::Exclude => repository
                .set_analytics_included("entry", false)
                .map(|found| {
                    assert!(found);
                }),
            Self::Clear => repository.clear(),
        }
    }
}

fn privacy_mutation_failure_and_retry(mutation: PrivacyMutation) {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    // Both the exact action and periodic policy maintenance must retain the
    // requested intent. A successful unrelated append is never a privacy retry.
    for maintenance_retry in [false, true] {
        let directory = TestDirectory::new();
        let mut repository = directory.repository();
        let settings = crate::types::Settings {
            analytics_enabled: true,
            ..crate::types::Settings::default()
        };
        repository.maintain(&settings).unwrap();
        repository
            .append(entry_with_analytics(Utc::now()), true, 30)
            .unwrap();
        super::storage::save_trainer_cache(
            &directory.0,
            &serde_json::json!({
                "schemaVersion": 1, "privacyGeneration": 1, "periods": []
            }),
        )
        .unwrap();

        let request = crate::application::trainer_ai::SessionRequest::begin(&format!(
            "history-privacy-{}",
            uuid::Uuid::new_v4()
        ))
        .unwrap();
        let cancelled = request.token.clone();
        let changed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = changed.clone();
        repository.hooks.sources_changed = Arc::new(move |change| {
            if change.kind == super::HistoryChangeKind::Privacy {
                // The injected consumer is isolated from other concurrent tests,
                // using the same cancellation token as real trainer requests.
                cancelled.cancel();
            }
            captured.lock().unwrap().push(change);
        });
        let failed = Arc::new(AtomicBool::new(true));
        let fail = failed.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let before_hook = request.token.clone();
        repository.hooks.before_private_write = Arc::new(move |directory| {
            counted.fetch_add(1, Ordering::SeqCst);
            assert!(
                before_hook.is_cancelled(),
                "privacy must revoke work before cache I/O"
            );
            if fail.load(Ordering::SeqCst) {
                return Err(crate::error::AppError::Config(
                    "privacy fixture unavailable".into(),
                ));
            }
            crate::application::trainer_report::invalidate_private_cache_at(directory)
        });

        assert!(mutation.apply(&repository).is_err());
        assert!(request.token.is_cancelled());
        assert!(changed
            .lock()
            .unwrap()
            .iter()
            .any(|change| change.kind == super::HistoryChangeKind::Sources));
        assert!(repository.list(true, 30).is_err());
        assert!(repository.pending_analysis_input("entry").is_err());
        assert!(repository
            .with_trainer_entries(&settings, |_| Ok(()))
            .is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);

        let mut late = entry_with_analytics(Utc::now());
        late.id = "late".into();
        assert!(repository.append(late, true, 30).is_err());
        assert!(repository
            .complete_analysis(
                "entry",
                SpeechSessionAnalysis {
                    word_count: 2,
                    filler_count: 0,
                    filler_density_per_100_words: 0.0,
                    repetition_count: 0,
                    self_correction_count: 0,
                    unfinished_count: 0,
                    findings: Vec::new(),
                }
            )
            .is_err());
        assert!(
            repository.list(true, 30).is_err(),
            "unrelated writes must not reopen sources"
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);

        failed.store(false, Ordering::SeqCst);
        if maintenance_retry {
            repository.maintain(&settings).unwrap();
        } else {
            mutation.apply(&repository).unwrap();
        }
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            2,
            "one cache preparation per private attempt"
        );
        let entries = repository.list(true, 30).unwrap();
        match mutation {
            PrivacyMutation::Delete | PrivacyMutation::Clear => assert!(entries.is_empty()),
            PrivacyMutation::Exclude => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].text, "final text");
                assert!(!entries[0].analytics_included);
                // Exclusion remains reversible; source access is filtered by
                // analytics_included while the history record stays intact.
                assert_eq!(entries[0].original_text.as_deref(), Some("original text"));
            }
        }
        assert!(repository
            .pending_analysis_input("entry")
            .unwrap()
            .is_none());
        if matches!(mutation, PrivacyMutation::Clear) {
            assert!(
                super::storage::load_trainer_cache::<serde_json::Value>(&directory.0)
                    .unwrap()
                    .is_none()
            );
        }
        drop(repository);
        let reopened = directory.repository();
        reopened.maintain(&settings).unwrap();
        let persisted = reopened.list(true, 30).unwrap();
        assert_eq!(persisted.len(), entries.len());
        assert!(!persisted.iter().any(|entry| entry.id == "late"));
        assert!(persisted.iter().all(|entry| !entry.analytics_included));
        let mut next = entry_with_analytics(Utc::now());
        next.id = "next".into();
        reopened.append(next, true, 30).unwrap();
        assert!(reopened.pending_analysis_input("next").unwrap().is_some());
    }
}

#[test]
fn failed_delete_revokes_work_blocks_unrelated_writes_and_preserves_retry_intent() {
    privacy_mutation_failure_and_retry(PrivacyMutation::Delete);
}

#[test]
fn failed_exclusion_revokes_work_blocks_unrelated_writes_and_preserves_retry_intent() {
    privacy_mutation_failure_and_retry(PrivacyMutation::Exclude);
}

#[test]
fn failed_clear_revokes_work_blocks_unrelated_writes_and_preserves_retry_intent() {
    privacy_mutation_failure_and_retry(PrivacyMutation::Clear);
}

#[test]
fn independent_repositories_emit_typed_session_changes_under_their_own_lock() {
    use std::sync::{Arc, Mutex};
    let first_directory = TestDirectory::new();
    let second_directory = TestDirectory::new();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let captured = changes.clone();
    let mut first = first_directory.repository();
    first.hooks.sources_changed = Arc::new(move |change| captured.lock().unwrap().push(change));
    let second = second_directory.repository();
    let settings = crate::types::Settings {
        analytics_enabled: true,
        ..crate::types::Settings::default()
    };
    first.maintain(&settings).unwrap();
    second.maintain(&settings).unwrap();
    first
        .append(entry_with_analytics(Utc::now()), true, 30)
        .unwrap();
    first.delete("entry").unwrap();
    assert!(changes
        .lock()
        .unwrap()
        .iter()
        .any(|change| change.kind == super::HistoryChangeKind::Session && change.ids == ["entry"]));
    assert!(second.list(true, 30).unwrap().is_empty());
}

fn entry_with_analytics(created_at: chrono::DateTime<chrono::Utc>) -> DictationHistoryEntry {
    DictationHistoryEntry {
        id: "entry".into(),
        text: "final text".into(),
        created_at,
        device: Some("CUDA".into()),
        metadata: None,
        analytics_included: true,
        original_text: Some("original text".into()),
        processing: Some(DictationProcessingMetadata {
            ai_mode: AiMode::Clean,
            detected_language: Some("ru".into()),
            transcribe_secs: Some(1.0),
            audio_secs: Some(2.0),
        }),
        analysis_status: DictationAnalysisStatus::Pending,
        analysis: None,
        analysis_error: None,
        recommendation_status: DictationAnalysisStatus::Disabled,
        recommendation: None,
        recommendation_error: None,
    }
}
