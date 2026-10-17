//! Pure retention, analysis transitions and period aggregation.

use super::*;

pub(super) fn apply_analytics_privacy_policy_to_entries(
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

pub(super) fn remove_entry(entries: &mut Vec<DictationHistoryEntry>, id: &str) -> bool {
    let previous_len = entries.len();
    entries.retain(|entry| entry.id != id);
    entries.len() != previous_len
}

pub(super) fn complete_analysis_in_entries(
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

pub(super) fn build_period_report(
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
