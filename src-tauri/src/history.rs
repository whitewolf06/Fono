//! Serialized repository for dictation history and its retention policy.

pub(crate) mod storage;

use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    sync::Arc,
};

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

mod analytics;
use analytics::{
    apply_analytics_privacy_policy_to_entries, build_period_report, complete_analysis_in_entries,
    remove_entry,
};
mod changes;
use changes::{PendingPrivacy, PrivacyWrite};
mod recommendations;
pub(crate) mod source;
pub use changes::{HistoryChangeKind, HistoryChanged, HistoryHooks};

type ChangeObserver = Arc<dyn Fn(HistoryChanged) + Send + Sync>;

pub struct HistoryRepository {
    store: Mutex<Option<storage::HistoryStore>>,
    directory: Option<PathBuf>,
    analytics_revision: AtomicU64,
    history_retention_days: AtomicU16,
    analytics_policy: Mutex<(bool, u16)>,
    privacy_blocked: AtomicBool,
    pending_privacy: Mutex<Option<PendingPrivacy>>,
    change_observer: Mutex<Option<ChangeObserver>>,
    hooks: HistoryHooks,
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
            pending_privacy: Mutex::new(None),
            change_observer: Mutex::new(None),
            hooks: HistoryHooks::default(),
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
                observer(HistoryChanged {
                    ids: Vec::new(),
                    kind: HistoryChangeKind::Durable,
                });
            }
        }
    }
}

impl HistoryRepository {
    pub fn new(hooks: HistoryHooks) -> Self {
        Self {
            hooks,
            ..Self::default()
        }
    }

    pub fn set_change_observer(&self, observer: ChangeObserver) {
        *self.change_observer.lock() = Some(observer);
    }

    fn sources_changed(&self) {
        (self.hooks.sources_changed)(HistoryChanged {
            ids: Vec::new(),
            kind: HistoryChangeKind::Sources,
        });
    }

    fn invalidate_session(&self, id: &str) {
        (self.hooks.sources_changed)(HistoryChanged {
            ids: vec![id.to_owned()],
            kind: HistoryChangeKind::Session,
        });
    }

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
        self.persist_with_privacy(&mut store, &[], PrivacyWrite::Clear)
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let mut store = self.access()?;
        let mut entries = self.privacy_entries(&store);
        if remove_entry(&mut entries, id) || self.has_pending_privacy() {
            self.persist_entries(&mut store, &entries, true)?;
            self.analytics_revision.fetch_add(1, Ordering::SeqCst);
            self.invalidate_session(id);
            self.sources_changed();
        }
        Ok(())
    }

    pub fn set_analytics_included(&self, id: &str, included: bool) -> AppResult<bool> {
        let mut store = self.access()?;
        let mut entries = self.privacy_entries(&store);
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        let timing_removed = !included
            && entry
                .metadata
                .as_mut()
                .is_some_and(|metadata| metadata.speech_timing.take().is_some());
        if entry.analytics_included == included {
            if !included && (timing_removed || self.has_pending_privacy()) {
                self.persist_entries(&mut store, &entries, true)?;
            }
            return Ok(true);
        }

        entry.analytics_included = included;
        if !included {
            self.persist_entries(&mut store, &entries, true)?;
        } else {
            self.persist_entries(&mut store, &entries, false)?;
        }
        self.analytics_revision.fetch_add(1, Ordering::SeqCst);
        self.invalidate_session(id);
        self.sources_changed();
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
        let entries = self.privacy_entries(&store);
        // Revocation also scrubs frozen derived fragments and JSON recovery copies
        // when every source entry has already lost its own analytics payload.
        self.persist_with_privacy(
            &mut store,
            &entries,
            if analytics_enabled {
                PrivacyWrite::Policy
            } else {
                PrivacyWrite::Sanitize
            },
        )
    }

    pub fn clear_analytics(&self) -> AppResult<()> {
        self.privacy_blocked.store(true, Ordering::SeqCst);
        let mut store = self.access()?;
        let mut entries = self.privacy_entries(&store);
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
        self.sources_changed();
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
        self.sources_changed();
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
        (self.hooks.sources_changed)(HistoryChanged {
            ids: Vec::new(),
            kind: HistoryChangeKind::Privacy,
        });
        self.sources_changed();
    }

    pub(crate) fn with_trainer_entries<T>(
        &self,
        _settings: &Settings,
        action: impl FnOnce(Vec<DictationHistoryEntry>) -> AppResult<T>,
    ) -> AppResult<T> {
        let store = self.read_access()?;
        action(store.entries.clone())
    }
}

pub fn next_id() -> String {
    let sequence = NEXT_HISTORY_ID.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{sequence}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    )
}

pub fn for_app(app: &tauri::AppHandle) -> Arc<HistoryRepository> {
    use tauri::Manager;
    app.state::<Arc<HistoryRepository>>().inner().clone()
}

#[cfg(test)]
mod tests;
