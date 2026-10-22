//! Context erasure is independent of transcripts, analytics and LLM variants.
use super::*;
use crate::types::ApplicationContext;

impl HistoryRepository {
    pub(crate) fn set_application_context_policy(&self, settings: &Settings) {
        let enabled = settings.history_enabled && settings.application_context_enabled;
        if self
            .application_context_enabled
            .swap(enabled, Ordering::SeqCst)
            != enabled
        {
            self.application_context_epoch
                .fetch_add(1, Ordering::SeqCst);
        }
    }
    pub(crate) fn application_context_permit(&self) -> Option<u64> {
        self.application_context_enabled
            .load(Ordering::SeqCst)
            .then(|| self.application_context_epoch.load(Ordering::SeqCst))
    }
    pub(crate) fn append_with_application_context(
        &self,
        mut entry: DictationHistoryEntry,
        enabled: bool,
        context: Option<(u64, ApplicationContext)>,
        analytics: bool,
        days: u16,
    ) -> AppResult<()> {
        let mut store = self.access()?;
        if let Some(metadata) = &mut entry.metadata {
            metadata.application_context = context.and_then(|(epoch, value)| {
                (enabled && self.application_context_permit() == Some(epoch)).then_some(value)
            });
        }
        apply_analytics_privacy_policy_to_entries(
            std::slice::from_mut(&mut entry),
            analytics,
            days,
            chrono::Utc::now(),
        );
        let mut entries = store.entries.clone();
        entries.insert(0, entry);
        self.persist_entries(&mut store, &entries, false)
    }
    pub(crate) fn clear_application_context(&self, id: Option<&str>) -> AppResult<()> {
        self.application_context_epoch
            .fetch_add(1, Ordering::SeqCst);
        let mut store = self.access()?;
        let mut entries = self.privacy_entries(&store);
        for entry in &mut entries {
            if id.map_or(true, |id| entry.id == id) {
                if let Some(metadata) = &mut entry.metadata {
                    metadata.application_context = None;
                }
            }
        }
        self.persist_entries(&mut store, &entries, true)
    }
}

#[cfg(test)]
mod tests;
