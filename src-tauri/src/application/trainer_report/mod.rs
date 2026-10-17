//! Frozen local speech reports. Source checks and cache writes share history's lock.

pub mod model;
#[cfg(test)]
mod tests;

pub use model::*;

pub(crate) use crate::history::source::{eligible, source_fingerprint};

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, AppResult},
    state,
    types::{DictationAnalysisStatus, DictationHistoryEntry, Settings, SpeechFindingKind},
};

pub const RULES_VERSION: &str = "speech-patterns-2";
const CACHE_SCHEMA_VERSION: u16 = 1;
const MAX_EXAMPLES: usize = 3;
const MAX_FRAGMENT_CHARS: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedPeriod {
    snapshot: TrainerSnapshot,
    ai_summary: Option<TrainerAiSummary>,
    #[serde(default)]
    privacy_invalidated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrainerCache {
    schema_version: u16,
    #[serde(default)]
    privacy_generation: u64,
    periods: Vec<CachedPeriod>,
}

impl Default for TrainerCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            privacy_generation: 0,
            periods: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct Period {
    days: u32,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
}

impl Period {
    fn new(days: u32, now: DateTime<Utc>) -> AppResult<Self> {
        if !matches!(days, 7 | 30 | 90) {
            return Err(AppError::Config("Выберите период 7, 30 или 90 дней".into()));
        }
        let midnight = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
        Ok(Self {
            days,
            from: midnight - Duration::days(i64::from(days - 1)),
            to: midnight + Duration::days(1),
        })
    }

    fn contains(self, entry: &DictationHistoryEntry) -> bool {
        entry.created_at >= self.from && entry.created_at < self.to
    }
}

/// Opening a saved report never reruns its analytics or its AI layer.
pub(crate) fn get_with_sources(
    repository: &crate::history::HistoryRepository,
    days: u32,
    refresh: bool,
    settings: &Settings,
) -> AppResult<(TrainerSnapshotState, Vec<DictationHistoryEntry>)> {
    repository.with_trainer_entries(settings, |entries| {
        let now = Utc::now();
        let mut cache = load_cache()?;
        let (report, sources, changed) =
            get_from_cache_with_sources(&mut cache, &entries, days, refresh, settings, now)?;
        if changed {
            state::save_trainer_cache_document(&cache)?;
        }
        Ok((report, sources))
    })
}

fn get_from_cache_with_sources(
    cache: &mut TrainerCache,
    entries: &[DictationHistoryEntry],
    days: u32,
    refresh: bool,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AppResult<(TrainerSnapshotState, Vec<DictationHistoryEntry>, bool)> {
    let (report, changed) = get_from_cache(cache, entries, days, refresh, settings, now)?;
    let sources = if report.stale {
        Vec::new()
    } else {
        eligible_sources(entries, Period::new(days, now)?, settings, now)
    };
    Ok((report, sources, changed))
}

/// Called immediately before submitting a consented payload; it does not refresh a stale report.
pub fn analysis_input(
    repository: &crate::history::HistoryRepository,
    days: u32,
    fingerprint: &str,
    settings: &Settings,
) -> AppResult<(TrainerSnapshot, Vec<DictationHistoryEntry>)> {
    repository.with_trainer_entries(settings, |entries| {
        let now = Utc::now();
        let period = Period::new(days, now)?;
        let cache = load_cache()?;
        let current = fingerprint_for(&entries, period, settings, now, cache.privacy_generation)?;
        let saved = cache
            .periods
            .iter()
            .find(|saved| saved.snapshot.days == days)
            .ok_or_else(|| AppError::Config("Сначала обновите локальный отчёт".into()))?;
        if current != fingerprint
            || saved.snapshot.fingerprint != current
            || saved.snapshot.rules_version != RULES_VERSION
            || saved.privacy_invalidated
        {
            return Err(AppError::Config(
                "Данные периода изменились. Обновите локальный отчёт".into(),
            ));
        }
        Ok((
            saved.snapshot.clone(),
            eligible_sources(&entries, period, settings, now),
        ))
    })
}

/// The source comparison and persistence are atomic against deletion/exclusion/analysis updates.
pub fn save_ai(
    repository: &crate::history::HistoryRepository,
    days: u32,
    expected_current_fp: &str,
    summary: TrainerAiSummary,
    settings: &Settings,
) -> AppResult<()> {
    repository.with_trainer_entries(settings, |entries| {
        let now = Utc::now();
        let period = Period::new(days, now)?;
        let mut cache = load_cache()?;
        let current = fingerprint_for(&entries, period, settings, now, cache.privacy_generation)?;
        insert_ai(&mut cache, days, expected_current_fp, &current, summary)?;
        state::save_trainer_cache_document(&cache)
    })
}

fn insert_ai(
    cache: &mut TrainerCache,
    days: u32,
    expected_current_fp: &str,
    current: &str,
    summary: TrainerAiSummary,
) -> AppResult<()> {
    let saved = cache
        .periods
        .iter_mut()
        .find(|saved| saved.snapshot.days == days)
        .ok_or_else(|| {
            AppError::Config("Локальный отчёт не найден. Обновите данные периода".into())
        })?;
    if current != expected_current_fp
        || saved.snapshot.fingerprint != current
        || saved.snapshot.rules_version != RULES_VERSION
        || summary.snapshot_fingerprint != current
        || saved.privacy_invalidated
    {
        return Err(AppError::Config(
            "Данные периода изменились. Результат ИИ не сохранён".into(),
        ));
    }
    saved.ai_summary = Some(summary);
    Ok(())
}

/// Keep numerical historical evidence, but remove every personal fragment and its backup.
pub(crate) fn invalidate_private_cache_at(directory: &std::path::Path) -> AppResult<()> {
    let mut cache = match crate::history::storage::load_trainer_cache::<TrainerCache>(directory) {
        Ok(Some(cache)) if cache.schema_version == CACHE_SCHEMA_VERSION => cache,
        _ => TrainerCache::default(),
    };
    sanitize_cache(&mut cache);
    crate::history::storage::save_trainer_cache(directory, &cache)
}

fn load_cache() -> AppResult<TrainerCache> {
    match state::load_trainer_cache_document::<TrainerCache>() {
        Ok(Some(mut cache)) if cache.schema_version == CACHE_SCHEMA_VERSION => {
            // A derived cache can contain only one report for each supported period.
            let mut days = BTreeSet::new();
            cache.periods.retain(|saved| {
                matches!(saved.snapshot.days, 7 | 30 | 90) && days.insert(saved.snapshot.days)
            });
            Ok(cache)
        }
        Ok(None) => Ok(TrainerCache::default()),
        Ok(Some(_)) | Err(_) => {
            // Old/incompatible derived schemas contain no authoritative history.
            state::purge_trainer_cache_document()?;
            Ok(TrainerCache::default())
        }
    }
}

fn sanitize_cache(cache: &mut TrainerCache) {
    cache.privacy_generation = cache.privacy_generation.saturating_add(1);
    for saved in &mut cache.periods {
        saved.snapshot.terms.clear();
        saved.snapshot.session_ids.clear();
        saved.ai_summary = None;
        saved.privacy_invalidated = true;
    }
}

fn get_from_cache(
    cache: &mut TrainerCache,
    entries: &[DictationHistoryEntry],
    days: u32,
    refresh: bool,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AppResult<(TrainerSnapshotState, bool)> {
    let period = Period::new(days, now)?;
    let current = fingerprint_for(entries, period, settings, now, cache.privacy_generation)?;
    let position = cache
        .periods
        .iter()
        .position(|saved| saved.snapshot.days == days);
    let needs_refresh = position.is_some_and(|position| {
        let saved = &cache.periods[position];
        saved.privacy_invalidated
            || saved.snapshot.fingerprint != current
            || saved.snapshot.rules_version != RULES_VERSION
    });
    let mut changed = false;
    let position = match position {
        Some(position) if !refresh || !needs_refresh => position,
        old_position => {
            let saved = CachedPeriod {
                snapshot: build_snapshot(entries, period, settings, now, current.clone()),
                ai_summary: None,
                privacy_invalidated: false,
            };
            changed = true;
            match old_position {
                Some(position) => {
                    cache.periods[position] = saved;
                    position
                }
                None => {
                    cache.periods.push(saved);
                    cache.periods.len() - 1
                }
            }
        }
    };
    let saved = &cache.periods[position];
    let stale = saved.privacy_invalidated
        || saved.snapshot.fingerprint != current
        || saved.snapshot.rules_version != RULES_VERSION;
    let mut snapshot = saved.snapshot.clone();
    // Never return a fragment/ID from an older source set, even before an explicit refresh.
    if stale {
        snapshot.terms.clear();
        snapshot.session_ids.clear();
    }
    Ok((
        TrainerSnapshotState {
            snapshot,
            current_fingerprint: current,
            stale,
            ai_summary: if stale {
                None
            } else {
                saved.ai_summary.clone()
            },
        },
        changed,
    ))
}

fn eligible_sources(
    entries: &[DictationHistoryEntry],
    period: Period,
    settings: &Settings,
    now: DateTime<Utc>,
) -> Vec<DictationHistoryEntry> {
    let mut sources = entries
        .iter()
        .filter(|entry| period.contains(entry) && eligible(entry, settings, now))
        .cloned()
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    sources
}

fn fingerprint_for(
    entries: &[DictationHistoryEntry],
    period: Period,
    settings: &Settings,
    now: DateTime<Utc>,
    privacy_generation: u64,
) -> AppResult<String> {
    let mut sources = entries
        .iter()
        .filter(|entry| period.contains(entry))
        .map(|entry| {
            Ok((
                &entry.id,
                source_fingerprint(entry)?,
                eligible(entry, settings, now),
            ))
        })
        .collect::<AppResult<Vec<_>>>()?;
    sources.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(&right.1)));
    hash_serialized(&(
        CACHE_SCHEMA_VERSION,
        RULES_VERSION,
        period.days,
        period.from,
        period.to,
        settings.history_enabled,
        settings.analytics_enabled,
        settings.speech_trainer_enabled,
        settings.analytics_retention_days,
        entries.len(),
        entries.iter().map(|entry| entry.created_at).min(),
        privacy_generation,
        sources,
    ))
}

fn hash_serialized(value: &impl Serialize) -> AppResult<String> {
    let digest = Sha256::digest(serde_json::to_vec(value)?);
    Ok(format!("{digest:x}"))
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn density(count: u32, words: u32) -> f32 {
    if words == 0 {
        0.0
    } else {
        count as f32 * 100.0 / words as f32
    }
}

fn counts(analysis: &crate::types::SpeechSessionAnalysis) -> TrainerCounts {
    TrainerCounts {
        filler: analysis.filler_count,
        repetition: analysis.repetition_count,
        self_correction: analysis.self_correction_count,
        unfinished: analysis.unfinished_count,
    }
}

fn normalized_term(kind: SpeechFindingKind, fragment: &str) -> (String, String) {
    let normalized = crate::speech_metrics::normalized_words(fragment).join(" ");
    let (prefix, label) = match kind {
        SpeechFindingKind::Filler => ("filler", normalized),
        SpeechFindingKind::SelfCorrection => ("self_correction", normalized),
        SpeechFindingKind::Repetition => (
            "repetition",
            normalized
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        ),
        SpeechFindingKind::Unfinished => ("unfinished", "unfinished".to_owned()),
    };
    (format!("{prefix}:{label}"), label)
}

fn build_snapshot(
    entries: &[DictationHistoryEntry],
    period: Period,
    settings: &Settings,
    now: DateTime<Utc>,
    fingerprint: String,
) -> TrainerSnapshot {
    let mut report = TrainerSnapshot {
        fingerprint,
        rules_version: RULES_VERSION.into(),
        days: period.days,
        from: timestamp(period.from),
        to: timestamp(period.to),
        generated_at: timestamp(now),
        included_sessions: 0,
        excluded_sessions: 0,
        pending_sessions: 0,
        unavailable_sessions: 0,
        total_words: 0,
        counts: TrainerCounts::default(),
        density: 0.0,
        terms: Vec::new(),
        daily: (0..period.days)
            .map(|index| TrainerDay {
                date: (period.from + Duration::days(i64::from(index)))
                    .date_naive()
                    .to_string(),
                sessions: 0,
                words: 0,
                counts: TrainerCounts::default(),
                density: 0.0,
            })
            .collect(),
        session_ids: Vec::new(),
        insufficient_data: true,
        history_limit: None,
        coverage_start: entries
            .iter()
            .map(|entry| entry.created_at)
            .min()
            .map(timestamp),
        retention_days: settings.analytics_retention_days,
    };
    let mut terms = BTreeMap::<String, TrainerTerm>::new();
    // Deterministic ordering also makes bounded examples reproducible after reopening.
    let mut ordered = entries
        .iter()
        .filter(|entry| period.contains(entry))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    for entry in ordered {
        if !entry.analytics_included {
            report.excluded_sessions += 1;
            continue;
        }
        if !eligible(entry, settings, now) {
            if settings.history_enabled
                && settings.analytics_enabled
                && settings.speech_trainer_enabled
                && entry.analysis_status == DictationAnalysisStatus::Pending
                && entry.original_text.is_some()
                && now.signed_duration_since(entry.created_at)
                    < Duration::days(i64::from(settings.analytics_retention_days))
            {
                report.pending_sessions += 1;
            } else {
                report.unavailable_sessions += 1;
            }
            continue;
        }
        // Recalculate only on first build/explicit refresh, using the current versioned rules.
        let analysis =
            crate::speech_metrics::analyze_russian_speech(entry.original_text.as_deref().unwrap());
        let session_counts = counts(&analysis);
        report.included_sessions += 1;
        report.total_words += analysis.word_count;
        report.counts.add(&session_counts);
        report.session_ids.push(entry.id.clone());
        let day_index = entry
            .created_at
            .date_naive()
            .signed_duration_since(period.from.date_naive())
            .num_days() as usize;
        let day = &mut report.daily[day_index];
        day.sessions += 1;
        day.words += analysis.word_count;
        day.counts.add(&session_counts);
        for finding in &analysis.findings {
            let (key, label) = normalized_term(finding.kind, &finding.fragment);
            let term = terms.entry(key.clone()).or_insert(TrainerTerm {
                key,
                label,
                kind: finding.kind,
                count: 0,
                density: 0.0,
                affected_sessions: 0,
                session_ids: Vec::new(),
                examples: Vec::new(),
            });
            term.count += 1;
            if !term.session_ids.contains(&entry.id) {
                term.affected_sessions += 1;
                term.session_ids.push(entry.id.clone());
                if term.examples.len() < MAX_EXAMPLES {
                    term.examples.push(TrainerExample {
                        session_id: entry.id.clone(),
                        fragment: finding.fragment.chars().take(MAX_FRAGMENT_CHARS).collect(),
                    });
                }
            }
        }
    }
    report.density = density(report.counts.total(), report.total_words);
    for day in &mut report.daily {
        day.density = density(day.counts.total(), day.words);
    }
    report.terms = terms.into_values().collect();
    for term in &mut report.terms {
        term.density = density(term.count, report.total_words);
    }
    report
        .terms
        .sort_by(|left, right| right.count.cmp(&left.count).then(left.key.cmp(&right.key)));
    report.insufficient_data = report.included_sessions < 3 || report.total_words < 100;
    report
}
