//! Conditional recommendation commits share the repository source lock.

use super::*;

impl HistoryRepository {
    fn recommendation_fingerprint(&self, entry: &DictationHistoryEntry) -> AppResult<String> {
        let source = source::source_fingerprint(entry)?;
        Ok(format!(
            "{}:{source}",
            self.analytics_revision.load(Ordering::SeqCst)
        ))
    }

    pub fn recommendation_input(
        &self,
        id: &str,
        settings: &Settings,
    ) -> AppResult<Option<(String, SpeechSessionAnalysis, String)>> {
        self.with_trainer_entries(settings, |entries| {
            let Some(entry) = entries.iter().find(|entry| {
                entry.id == id
                    && entry.recommendation_status == DictationAnalysisStatus::Pending
                    && source::eligible(entry, settings, chrono::Utc::now())
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
            || !source::eligible(entry, settings, chrono::Utc::now())
            || self.recommendation_fingerprint(entry)? != expected_source_fp
        {
            return Ok(false);
        }
        apply(entry);
        self.persist_entries(&mut store, &entries, false)?;
        Ok(true)
    }
    pub fn complete_recommendation_if_current(
        &self,
        id: &str,
        expected_source_fp: &str,
        recommendation: crate::llm::SpeechLlmRecommendation,
        settings: &Settings,
    ) -> AppResult<bool> {
        self.update_recommendation_if_current(id, expected_source_fp, settings, |entry| {
            entry.recommendation = Some(recommendation);
            entry.recommendation_error = None;
            entry.recommendation_status = DictationAnalysisStatus::Ready;
        })
    }

    pub fn mark_recommendation_failed_if_current(
        &self,
        id: &str,
        expected_source_fp: &str,
        reason: &str,
        settings: &Settings,
    ) -> AppResult<bool> {
        self.update_recommendation_if_current(id, expected_source_fp, settings, |entry| {
            entry.recommendation_error = Some(reason.to_owned());
            entry.recommendation_status = DictationAnalysisStatus::Failed;
        })
    }
}
