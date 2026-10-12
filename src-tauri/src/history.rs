//! Serialized repository for dictation history and its retention policy.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{
    error::AppResult,
    state,
    types::{
        DictationAnalysisStatus, DictationHistoryEntry, Settings, SpeechDailyTrend,
        SpeechPeriodReport, SpeechSessionAnalysis,
    },
};

const MAX_HISTORY_ENTRIES: usize = 200;
static NEXT_HISTORY_ID: AtomicU64 = AtomicU64::new(0);
static HISTORY: Lazy<HistoryRepository> = Lazy::new(HistoryRepository::default);

#[derive(Default)]
pub struct HistoryRepository {
    write_lock: Mutex<()>,
    analytics_revision: AtomicU64,
}

impl HistoryRepository {
    pub fn list(
        &self,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<Vec<DictationHistoryEntry>> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        ) {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
            self.privacy_changed();
        }
        Ok(entries)
    }

    pub fn append(
        &self,
        entry: DictationHistoryEntry,
        analytics_enabled: bool,
        analytics_retention_days: u16,
    ) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        let expired = apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        );
        entries.insert(0, entry);
        let dropped_ids = entries
            .iter()
            .skip(MAX_HISTORY_ENTRIES)
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        entries.truncate(MAX_HISTORY_ENTRIES);
        if expired || !dropped_ids.is_empty() {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
        } else {
            state::save_history_document(&entries)?;
        }
        if expired {
            self.privacy_changed();
        }
        for id in dropped_ids {
            crate::application::trainer_ai::invalidate_session(&id);
        }
        crate::application::trainer_ai::sources_changed();
        Ok(())
    }

    pub fn clear(&self) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        crate::application::trainer_report::purge()?;
        state::save_history_document_private::<DictationHistoryEntry>(&[])?;
        self.privacy_changed();
        Ok(())
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if remove_entry(&mut entries, id) {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
            self.analytics_revision.fetch_add(1, Ordering::SeqCst);
            crate::application::trainer_ai::invalidate_session(id);
            crate::application::trainer_ai::sources_changed();
        }
        Ok(())
    }

    pub fn set_analytics_included(&self, id: &str, included: bool) -> AppResult<bool> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.analytics_included == included {
            return Ok(true);
        }

        entry.analytics_included = included;
        if !included {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
        } else {
            state::save_history_document(&entries)?;
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
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if apply_analytics_privacy_policy_to_entries(
            &mut entries,
            analytics_enabled,
            analytics_retention_days,
            chrono::Utc::now(),
        ) {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
            self.privacy_changed();
        }
        Ok(())
    }

    pub fn pending_analysis_ids(&self) -> AppResult<Vec<String>> {
        let _guard = self.write_lock.lock();
        let entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
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
        let _guard = self.write_lock.lock();
        let entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        Ok(entries.into_iter().find_map(|entry| {
            (entry.id == id
                && entry.analytics_included
                && entry.analysis_status == DictationAnalysisStatus::Pending)
                .then_some(entry.original_text)
                .flatten()
        }))
    }

    pub fn complete_analysis(&self, id: &str, analysis: SpeechSessionAnalysis) -> AppResult<bool> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        let completed = complete_analysis_in_entries(&mut entries, id, analysis);
        if !completed {
            return Ok(false);
        }
        state::save_history_document(&entries)?;
        crate::application::trainer_ai::sources_changed();
        Ok(true)
    }

    pub fn mark_analysis_failed(&self, id: &str, reason: &str) -> AppResult<bool> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        if entry.analysis_status != DictationAnalysisStatus::Pending {
            return Ok(false);
        }

        entry.analysis_status = DictationAnalysisStatus::Failed;
        entry.analysis_error = Some(reason.to_owned());
        state::save_history_document(&entries)?;
        crate::application::trainer_ai::sources_changed();
        Ok(true)
    }

    pub fn session_analysis(&self, id: &str) -> AppResult<Option<SpeechSessionAnalysis>> {
        let _guard = self.write_lock.lock();
        let entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
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
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
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
        state::save_history_document(&entries)?;
        Ok(true)
    }

    pub fn mark_recommendation_failed(&self, id: &str, reason: &str) -> AppResult<bool> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
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
        state::save_history_document(&entries)?;
        Ok(true)
    }

    pub fn period_report(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<SpeechPeriodReport> {
        let _guard = self.write_lock.lock();
        let entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        Ok(build_period_report(entries, from, to))
    }

    fn privacy_changed(&self) {
        self.analytics_revision.fetch_add(1, Ordering::SeqCst);
        crate::application::trainer_ai::invalidate_all_sessions();
        crate::application::trainer_ai::sources_changed();
    }

    fn with_trainer_entries<T>(
        &self,
        settings: &Settings,
        action: impl FnOnce(Vec<DictationHistoryEntry>) -> AppResult<T>,
    ) -> AppResult<T> {
        let _guard = self.write_lock.lock();
        let mut entries: Vec<DictationHistoryEntry> = state::load_history_document()?;
        if apply_analytics_privacy_policy_to_entries(
            &mut entries,
            settings.analytics_enabled && settings.history_enabled,
            settings.analytics_retention_days,
            chrono::Utc::now(),
        ) {
            crate::application::trainer_report::invalidate_private_cache()?;
            state::save_history_document_private(&entries)?;
            self.privacy_changed();
        }
        action(entries)
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
            Ok(Some((
                entry.original_text.clone().unwrap(),
                entry.analysis.clone().unwrap(),
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
        self.with_trainer_entries(settings, |mut entries| {
            let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
                return Ok(false);
            };
            if entry.recommendation_status != DictationAnalysisStatus::Pending
                || !crate::application::trainer_report::eligible(
                    entry,
                    settings,
                    chrono::Utc::now(),
                )
                || self.recommendation_fingerprint(entry)? != expected_source_fp
            {
                return Ok(false);
            }
            apply(entry);
            state::save_history_document(&entries)?;
            Ok(true)
        })
    }
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

pub fn apply_analytics_privacy_policy(
    analytics_enabled: bool,
    analytics_retention_days: u16,
) -> AppResult<()> {
    HISTORY.apply_analytics_privacy_policy(analytics_enabled, analytics_retention_days)
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
