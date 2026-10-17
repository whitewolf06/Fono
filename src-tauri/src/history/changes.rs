//! Explicit source invalidation and fallible pre-commit privacy boundary.

use crate::error::AppResult;
use std::{path::Path, sync::Arc};

use super::{storage, HistoryRepository};
use crate::types::DictationHistoryEntry;
use std::sync::atomic::Ordering;

pub(super) struct PendingPrivacy {
    entries: Vec<DictationHistoryEntry>,
    clear_cache: bool,
}

#[derive(Clone, Copy)]
pub(super) enum PrivacyWrite {
    Ordinary,
    Policy,
    Sanitize,
    Clear,
}

impl HistoryRepository {
    pub(super) fn has_pending_privacy(&self) -> bool {
        self.pending_privacy.lock().is_some()
    }

    /// Private retries preserve the requested deletion/exclusion, even when a
    /// policy-maintenance retry arrives before the original user action.
    pub(super) fn privacy_entries(
        &self,
        store: &storage::HistoryStore,
    ) -> Vec<DictationHistoryEntry> {
        self.pending_privacy
            .lock()
            .as_ref()
            .map_or_else(|| store.entries.clone(), |pending| pending.entries.clone())
    }

    pub(super) fn persist_entries(
        &self,
        store: &mut storage::HistoryStore,
        entries: &[DictationHistoryEntry],
        private: bool,
    ) -> AppResult<()> {
        self.persist_with_privacy(
            store,
            entries,
            if private {
                PrivacyWrite::Sanitize
            } else {
                PrivacyWrite::Ordinary
            },
        )
    }

    pub(super) fn persist_with_privacy(
        &self,
        store: &mut storage::HistoryStore,
        entries: &[DictationHistoryEntry],
        privacy: PrivacyWrite,
    ) -> AppResult<()> {
        let requested = !matches!(privacy, PrivacyWrite::Ordinary);
        // An unrelated append/analysis completion must not acknowledge an
        // unsuccessful privacy operation and reopen its old source payload.
        if !requested && self.privacy_blocked.load(Ordering::SeqCst) {
            return Err(crate::error::AppError::Config(
                "History privacy update must be retried".into(),
            ));
        }
        let mut retained = entries.to_vec();
        let now = chrono::Utc::now();
        let (enabled, days) = *self.analytics_policy.lock();
        let expired_analytics =
            super::apply_analytics_privacy_policy_to_entries(&mut retained, enabled, days, now);
        retained.retain(|entry| {
            now.signed_duration_since(entry.created_at)
                < chrono::Duration::days(i64::from(
                    self.history_retention_days.load(Ordering::SeqCst),
                ))
        });
        let private = matches!(privacy, PrivacyWrite::Sanitize | PrivacyWrite::Clear)
            || self.has_pending_privacy()
            || expired_analytics
            || retained.len() != entries.len();
        if private {
            let clear_cache = matches!(privacy, PrivacyWrite::Clear)
                || self
                    .pending_privacy
                    .lock()
                    .as_ref()
                    .is_some_and(|pending| pending.clear_cache);
            *self.pending_privacy.lock() = Some(PendingPrivacy {
                entries: retained.clone(),
                clear_cache,
            });
            self.privacy_blocked.store(true, Ordering::SeqCst);
            // Revoke in-flight work before any fallible cache or disk write.
            self.privacy_changed();
            (self.hooks.before_private_write)(&store.directory)?;
            if clear_cache {
                storage::purge_trainer_cache(&store.directory)?;
            }
        }
        let revision = store.revision;
        store.persist(&retained, private)?;
        *self.pending_privacy.lock() = None;
        self.privacy_blocked.store(false, Ordering::SeqCst);
        if !private && store.revision != revision {
            self.sources_changed();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryChangeKind {
    Sources,
    Session,
    Privacy,
    Durable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryChanged {
    /// Empty means the complete source snapshot; IDs narrow session invalidation.
    pub ids: Vec<String>,
    pub kind: HistoryChangeKind,
}

pub type PrivacyRewrite = Arc<dyn Fn(&Path) -> AppResult<()> + Send + Sync>;

/// The source callback is synchronous under the repository lock: it invalidates
/// in-flight work atomically and must never read history. Durable observers run
/// after that lock is released and may obtain a fresh snapshot.
pub struct HistoryHooks {
    pub before_private_write: PrivacyRewrite,
    pub sources_changed: Arc<dyn Fn(HistoryChanged) + Send + Sync>,
}
impl Default for HistoryHooks {
    fn default() -> Self {
        Self {
            // A standalone repository without a report consumer safely discards derived caches.
            before_private_write: Arc::new(super::storage::purge_trainer_cache),
            sources_changed: Arc::new(|_| {}),
        }
    }
}
