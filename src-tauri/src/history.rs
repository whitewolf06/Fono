//! Serialized repository for dictation history and its retention policy.

pub(crate) mod storage;

use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    sync::Arc,
};

use once_cell::sync::Lazy;
use parking_lot::{Mutex, MutexGuard};

use crate::{
    error::{AppError, AppResult},
    state,
    types::{
        DictationAnalysisStatus, DictationHistoryEntry, Settings, SpeechDailyTrend,
        SpeechPeriodReport, SpeechSessionAnalysis,
    },
};

static NEXT_HISTORY_ID: AtomicU64 = AtomicU64::new(0);
static HISTORY: Lazy<HistoryRepository> = Lazy::new(HistoryRepository::default);

type ChangeObserver = Arc<dyn Fn() + Send + Sync>;

pub struct HistoryRepository {
    store: Mutex<Option<storage::HistoryStore>>,
    directory: Option<PathBuf>,
    analytics_revision: AtomicU64,
    history_retention_days: AtomicU16,
    analytics_policy: Mutex<(bool, u16)>,
    privacy_blocked: AtomicBool,
    change_observer: Mutex<Option<ChangeObserver>>,
}

impl Default for HistoryRepository {
    fn default() -> Self {
        Self {
            store: Mutex::new(None),
            directory: None,
            analytics_revision: AtomicU64::new(0),
            history_retention_days: AtomicU16::new(365),
            analytics_policy: Mutex::new((false, 30)),
            privacy_blocked: AtomicBool::new(false),
            change_observer: Mutex::new(None),
        }
    }
}

/// Notify durable changes only after releasing storage's lock, so a new snapshot
/// may safely be requested by the event consumer.
struct StoreAccess<'a> {
    guard: Option<MutexGuard<'a, Option<storage::HistoryStore>>>,
    revision: u64,
    observer: Option<ChangeObserver>,
}
impl Deref for StoreAccess<'_> {
    type Target = storage::HistoryStore;
    fn deref(&self) -> &Self::Target {
        self.guard
            .as_ref()
            .and_then(|guard| guard.as_ref())
            .expect("initialized history guard")
    }
}
impl DerefMut for StoreAccess<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .and_then(|guard| guard.as_mut())
            .expect("initialized history guard")
    }
}
impl Drop for StoreAccess<'_> {
    fn drop(&mut self) {
        let changed = self
            .guard
            .as_ref()
            .and_then(|guard| guard.as_ref())
            .is_some_and(|store| store.revision != self.revision);
        drop(self.guard.take());
        if changed {
            if let Some(observer) = &self.observer {
                observer();
            }
        }
    }
}

impl HistoryRepository {
    fn access(&self) -> AppResult<StoreAccess<'_>> {
        let mut guard = self.store.lock();
        if guard.is_none() {
            let directory = match &self.directory {
                Some(directory) => directory.clone(),
                None => state::app_data_dir()?,
            };
            *guard = Some(storage::HistoryStore::open(directory)?);
        }
        let revision = guard.as_ref().map_or(0, |store| store.revision);
        Ok(StoreAccess {
            guard: Some(guard),
            revision,
            observer: self.change_observer.lock().clone(),
        })
    }

    fn read_access(&self) -> AppResult<StoreAccess<'_>> {
        let access = self.access()?;
        if self.privacy_blocked.load(Ordering::SeqCst) {
            return Err(AppError::Config(
                "History privacy update must be retried".into(),
            ));
        }
        Ok(access)
    }

    fn persist_entries(
        &self,
        store: &mut storage::HistoryStore,
        entries: &[DictationHistoryEntry],
        private: bool,
    ) -> AppResult<()> {
        let mut retained = entries.to_vec();
        let now = chrono::Utc::now();
        let (enabled, days) = *self.analytics_policy.lock();
        let expired_analytics =
            apply_analytics_privacy_policy_to_entries(&mut retained, enabled, days, now);
        retained.retain(|entry| {
            now.signed_duration_since(entry.created_at)
                < chrono::Duration::days(i64::from(
                    self.history_retention_days.load(Ordering::SeqCst),
                ))
        });
        let removed = retained.len() != entries.len();
        if private || expired_analytics || removed {
            // A failed privacy write must not expose the old in-memory payload.
            // Keep it internally for a durable retry, while denying all source reads.
            self.privacy_blocked.store(true, Ordering::SeqCst);
            crate::application::trainer_report::invalidate_private_cache_at(&store.directory)?;
        }
        let revision = store.revision;
        store.persist(&retained, private || expired_analytics || removed)?;
        self.privacy_blocked.store(false, Ordering::SeqCst);
        if expired_analytics || removed {
            self.privacy_changed();
        } else if private || store.revision != revision {
            crate::application::trainer_ai::sources_changed();
        }
        Ok(())
    }

    pub fn list(
        &self,
        _analytics_enabled: bool,
        _analytics_retention_days: u16,
    ) -> AppResult<Vec<DictationHistoryEntry>> {
        // Startup/maintenance and mutations enforce policy; ordinary reads perform no I/O.
        Ok(self.read_access()?.entries.clone())
    }

    pub fn append(
        &self,
        entry: DictationHistoryEntry,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<()> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        // The operation's older settings snapshot may narrow consent, never re-enable
        // data collection after settings/maintenance have revoked it.
        let mut entry = entry;
        apply_analytics_privacy_policy_to_entries(
            std::slice::from_mut(&mut entry),
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        );
        entries.insert(0, entry);
        self.persist_entries(&mut store, &entries, false)?;
        Ok(())
    }

    pub fn clear(&self) -> AppResult<()> {
        let mut store = self.access()?;
        storage::purge_trainer_cache(&store.directory)?;
        self.persist_entries(&mut store, &[], true)?;
        self.privacy_changed();
        Ok(())
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        if remove_entry(&mut entries, id) {
            crate::application::trainer_report::invalidate_private_cache_at(&store.directory)?;
            self.persist_entries(&mut store, &entries, true)?;
            self.analytics_revision.fetch_add(1, Ordering::SeqCst);
            crate::application::trainer_ai::invalidate_session(id);
            crate::application::trainer_ai::sources_changed();
        }
        Ok(())
    }

    pub fn set_analytics_included(&self, id: &str, included: bool) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.analytics_included == included {
            return Ok(true);
        }

        entry.analytics_included = included;
        if !included {
            crate::application::trainer_report::invalidate_private_cache_at(&store.directory)?;
            self.persist_entries(&mut store, &entries, true)?;
        } else {
            self.persist_entries(&mut store, &entries, false)?;
        }
        self.analytics_revision.fetch_add(1, Ordering::SeqCst);
        crate::application::trainer_ai::invalidate_session(id);
        crate::application::trainer_ai::sources_changed();
        Ok(true)
    }

    pub fn apply_analytics_privacy_policy(
        &self,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<()> {
        // Also fail closed if opening/migrating the database fails before the
        // persistence helper can mark a pending privacy rewrite.
        self.privacy_blocked.store(true, Ordering::SeqCst);
        *self.analytics_policy.lock() = (analytics_enabled, analytics_retention_days);
        let mut store = self.access()?;
        let entries = store.entries.clone();
        // Revocation also scrubs frozen derived fragments and JSON recovery copies
        // when every source entry has already lost its own analytics payload.
        self.persist_entries(&mut store, &entries, !analytics_enabled)
    }

    pub fn clear_analytics(&self) -> AppResult<()> {
        self.privacy_blocked.store(true, Ordering::SeqCst);
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        apply_analytics_privacy_policy_to_entries(&mut entries, false, 0, chrono::Utc::now());
        self.persist_entries(&mut store, &entries, true)?;
        self.privacy_changed();
        Ok(())
    }

    pub fn maintain(&self, settings: &Settings) -> AppResult<()> {
        self.privacy_blocked.store(true, Ordering::SeqCst);
        if !(1..=3650).contains(&settings.history_retention_days)
            || (settings.analytics_enabled
                && !(1..=365).contains(&settings.analytics_retention_days))
        {
            return Err(AppError::Config("Invalid history retention policy".into()));
        }
        self.history_retention_days
            .store(settings.history_retention_days, Ordering::SeqCst);
        self.apply_analytics_privacy_policy(
            settings.analytics_enabled && settings.history_enabled,
            settings.analytics_retention_days,
        )
    }

    pub fn pending_analysis_ids(&self) -> AppResult<Vec<String>> {
        let store = self.read_access()?;
        let entries = store.entries.clone();
        Ok(entries
            .into_iter()
            .filter(|entry| {
                entry.analysis_status == DictationAnalysisStatus::Pending
                    && entry.analytics_included
                    && entry.original_text.is_some()
            })
            .map(|entry| entry.id)
            .collect())
    }

    pub fn pending_analysis_input(&self, id: &str) -> AppResult<Option<String>> {
        let store = self.read_access()?;
        let entries = store.entries.clone();
        Ok(entries.into_iter().find_map(|entry| {
            (entry.id == id
                && entry.analytics_included
                && entry.analysis_status == DictationAnalysisStatus::Pending)
                .then_some(entry.original_text)
                .flatten()
        }))
    }

    pub fn complete_analysis(&self, id: &str, analysis: SpeechSessionAnalysis) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let completed = complete_analysis_in_entries(&mut entries, id, analysis);
        if !completed {
            return Ok(false);
        }
        self.persist_entries(&mut store, &entries, false)?;
        crate::application::trainer_ai::sources_changed();
        Ok(true)
    }

    pub fn mark_analysis_failed(&self, id: &str, reason: &str) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.analysis_status != DictationAnalysisStatus::Pending {
            return Ok(false);
        }

        entry.analysis_status = DictationAnalysisStatus::Failed;
        entry.analysis_error = Some(reason.to_owned());
        self.persist_entries(&mut store, &entries, false)?;
        crate::application::trainer_ai::sources_changed();
        Ok(true)
    }

    pub fn session_analysis(&self, id: &str) -> AppResult<Option<SpeechSessionAnalysis>> {
        let store = self.read_access()?;
        let entries = store.entries.clone();
        Ok(entries
            .into_iter()
            .find(|entry| {
                entry.id == id
                    && entry.analytics_included
                    && entry.analysis_status == DictationAnalysisStatus::Ready
                    && entry.original_text.is_some()
            })
            .and_then(|entry| entry.analysis))
    }

    pub fn complete_recommendation(
        &self,
        id: &str,
        recommendation: crate::llm::SpeechLlmRecommendation,
    ) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.analysis_status != DictationAnalysisStatus::Ready
            || !entry.analytics_included
            || entry.original_text.is_none()
            || entry.recommendation_status != DictationAnalysisStatus::Pending
        {
            return Ok(false);
        }
        entry.recommendation = Some(recommendation);
        entry.recommendation_error = None;
        entry.recommendation_status = DictationAnalysisStatus::Ready;
        self.persist_entries(&mut store, &entries, false)?;
        Ok(true)
    }

    pub fn mark_recommendation_failed(&self, id: &str, reason: &str) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if !entry.analytics_included
            || entry.analysis_status != DictationAnalysisStatus::Ready
            || entry.original_text.is_none()
            || entry.recommendation_status != DictationAnalysisStatus::Pending
        {
            return Ok(false);
        }
        entry.recommendation_status = DictationAnalysisStatus::Failed;
        entry.recommendation_error = Some(reason.to_owned());
        self.persist_entries(&mut store, &entries, false)?;
        Ok(true)
    }

    pub fn period_report(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<SpeechPeriodReport> {
        let store = self.read_access()?;
        let entries = store.entries.clone();
        Ok(build_period_report(entries, from, to))
    }

    fn privacy_changed(&self) {
        self.analytics_revision.fetch_add(1, Ordering::SeqCst);
        crate::application::trainer_ai::invalidate_all_sessions();
        crate::application::trainer_ai::sources_changed();
    }

    fn with_trainer_entries<T>(
        &self,
        _settings: &Settings,
        action: impl FnOnce(Vec<DictationHistoryEntry>) -> AppResult<T>,
    ) -> AppResult<T> {
        let store = self.read_access()?;
        action(store.entries.clone())
    }

    fn recommendation_fingerprint(&self, entry: &DictationHistoryEntry) -> AppResult<String> {
        let source = crate::application::trainer_report::source_fingerprint(entry)?;
        Ok(format!(
            "{}:{source}",
            self.analytics_revision.load(Ordering::SeqCst)
        ))
    }

    fn recommendation_input(
        &self,
        id: &str,
        settings: &Settings,
    ) -> AppResult<Option<(String, SpeechSessionAnalysis, String)>> {
        self.with_trainer_entries(settings, |entries| {
            let Some(entry) = entries.iter().find(|entry| {
                entry.id == id
                    && entry.recommendation_status == DictationAnalysisStatus::Pending
                    && crate::application::trainer_report::eligible(
                        entry,
                        settings,
                        chrono::Utc::now(),
                    )
            }) else {
                return Ok(None);
            };
            let (Some(original_text), Some(analysis)) = (&entry.original_text, &entry.analysis)
            else {
                return Ok(None);
            };
            Ok(Some((
                original_text.clone(),
                analysis.clone(),
                self.recommendation_fingerprint(entry)?,
            )))
        })
    }

    fn update_recommendation_if_current(
        &self,
        id: &str,
        expected_source_fp: &str,
        settings: &Settings,
        apply: impl FnOnce(&mut DictationHistoryEntry),
    ) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = store.entries.clone();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.recommendation_status != DictationAnalysisStatus::Pending
            || !crate::application::trainer_report::eligible(entry, settings, chrono::Utc::now())
            || self.recommendation_fingerprint(entry)? != expected_source_fp
        {
            return Ok(false);
        }
        apply(entry);
        self.persist_entries(&mut store, &entries, false)?;
        Ok(true)
    }
}

pub fn set_change_observer(observer: ChangeObserver) {
    *HISTORY.change_observer.lock() = Some(observer);
}
pub fn maintain(settings: &Settings) -> AppResult<()> {
    HISTORY.maintain(settings)
}

pub fn start_maintenance(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
            let Ok(lease) = crate::application::updates::activity::lease() else {
                continue;
            };
            let transaction = crate::application::updates::settings_transaction(&app).await;
            let settings = app.state::<state::AppState>().settings();
            let result = tauri::async_runtime::spawn_blocking(move || {
                let _lease = lease;
                let _transaction = transaction;
                maintain(&settings)
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                tracing::warn!("history retention maintenance failed");
            }
        }
    });
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

pub fn set_analytics_included(id: &str, included: bool) -> AppResult<bool> {
    HISTORY.set_analytics_included(id, included)
}

pub fn clear_analytics() -> AppResult<()> {
    HISTORY.clear_analytics()
}

pub fn pending_analysis_ids() -> AppResult<Vec<String>> {
    HISTORY.pending_analysis_ids()
}

pub fn pending_analysis_input(id: &str) -> AppResult<Option<String>> {
    HISTORY.pending_analysis_input(id)
}

pub fn complete_analysis(id: &str, analysis: SpeechSessionAnalysis) -> AppResult<bool> {
    HISTORY.complete_analysis(id, analysis)
}

pub fn mark_analysis_failed(id: &str, reason: &str) -> AppResult<bool> {
    HISTORY.mark_analysis_failed(id, reason)
}

pub fn session_analysis(id: &str) -> AppResult<Option<SpeechSessionAnalysis>> {
    HISTORY.session_analysis(id)
}

pub(crate) fn with_trainer_entries<T>(
    settings: &Settings,
    action: impl FnOnce(Vec<DictationHistoryEntry>) -> AppResult<T>,
) -> AppResult<T> {
    HISTORY.with_trainer_entries(settings, action)
}

pub fn recommendation_input(
    id: &str,
    settings: &Settings,
) -> AppResult<Option<(String, SpeechSessionAnalysis, String)>> {
    HISTORY.recommendation_input(id, settings)
}

pub fn complete_recommendation_if_current(
    id: &str,
    expected_source_fp: &str,
    recommendation: crate::llm::SpeechLlmRecommendation,
    settings: &Settings,
) -> AppResult<bool> {
    HISTORY.update_recommendation_if_current(id, expected_source_fp, settings, |entry| {
        entry.recommendation = Some(recommendation);
        entry.recommendation_error = None;
        entry.recommendation_status = DictationAnalysisStatus::Ready;
    })
}

pub fn mark_recommendation_failed_if_current(
    id: &str,
    expected_source_fp: &str,
    reason: &str,
    settings: &Settings,
) -> AppResult<bool> {
    HISTORY.update_recommendation_if_current(id, expected_source_fp, settings, |entry| {
        entry.recommendation_error = Some(reason.to_owned());
        entry.recommendation_status = DictationAnalysisStatus::Failed;
    })
}

pub fn complete_recommendation(
    id: &str,
    recommendation: crate::llm::SpeechLlmRecommendation,
) -> AppResult<bool> {
    HISTORY.complete_recommendation(id, recommendation)
}

pub fn mark_recommendation_failed(id: &str, reason: &str) -> AppResult<bool> {
    HISTORY.mark_recommendation_failed(id, reason)
}

pub fn period_report(
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> AppResult<SpeechPeriodReport> {
    HISTORY.period_report(from, to)
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

fn complete_analysis_in_entries(
    entries: &mut [DictationHistoryEntry],
    id: &str,
    analysis: SpeechSessionAnalysis,
) -> bool {
    let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
        return false;
    };
    if !entry.analytics_included
        || entry.analysis_status != DictationAnalysisStatus::Pending
        || entry.original_text.is_none()
    {
        return false;
    }

    entry.analysis = Some(analysis);
    entry.analysis_error = None;
    entry.analysis_status = DictationAnalysisStatus::Ready;
    true
}

fn build_period_report(
    entries: Vec<DictationHistoryEntry>,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> SpeechPeriodReport {
    let mut report = SpeechPeriodReport {
        from,
        to,
        analyzed_sessions: 0,
        total_words: 0,
        filler_count: 0,
        repetition_count: 0,
        self_correction_count: 0,
        unfinished_count: 0,
        filler_density_per_100_words: 0.0,
        daily: Vec::new(),
    };
    let mut daily = BTreeMap::<chrono::NaiveDate, SpeechDailyTrend>::new();

    for entry in entries.into_iter().filter(|entry| {
        entry.analytics_included
            && entry.analysis_status == DictationAnalysisStatus::Ready
            && entry.original_text.is_some()
            && entry.created_at >= from
            && entry.created_at <= to
    }) {
        let Some(analysis) = entry.analysis else {
            continue;
        };
        report.analyzed_sessions += 1;
        report.total_words += analysis.word_count;
        report.filler_count += analysis.filler_count;
        report.repetition_count += analysis.repetition_count;
        report.self_correction_count += analysis.self_correction_count;
        report.unfinished_count += analysis.unfinished_count;

        let trend = daily
            .entry(entry.created_at.date_naive())
            .or_insert(SpeechDailyTrend {
                date: entry.created_at.date_naive(),
                sessions: 0,
                words: 0,
                filler_count: 0,
                repetition_count: 0,
                self_correction_count: 0,
                unfinished_count: 0,
            });
        trend.sessions += 1;
        trend.words += analysis.word_count;
        trend.filler_count += analysis.filler_count;
        trend.repetition_count += analysis.repetition_count;
        trend.self_correction_count += analysis.self_correction_count;
        trend.unfinished_count += analysis.unfinished_count;
    }

    if report.total_words > 0 {
        report.filler_density_per_100_words =
            report.filler_count as f32 * 100.0 / report.total_words as f32;
    }
    report.daily = daily.into_values().collect();
    report
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
        SpeechSessionAnalysis,
    };

    use super::{
        apply_analytics_privacy_policy_to_entries, build_period_report,
        complete_analysis_in_entries, next_id, remove_entry,
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
        *repository.change_observer.lock() = Some(Arc::new(move || {
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
}
