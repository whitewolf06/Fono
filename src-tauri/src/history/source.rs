//! Stable, content-private eligibility and source identity shared by consumers.

use crate::{
    error::AppResult,
    types::{DictationAnalysisStatus, DictationHistoryEntry, Settings},
};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

pub(crate) fn eligible(
    entry: &DictationHistoryEntry,
    settings: &Settings,
    now: DateTime<Utc>,
) -> bool {
    settings.history_enabled
        && settings.analytics_enabled
        && settings.speech_trainer_enabled
        && entry.analytics_included
        && entry.analysis_status == DictationAnalysisStatus::Ready
        && entry.analysis.is_some()
        && entry
            .original_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
        && now.signed_duration_since(entry.created_at)
            < Duration::days(i64::from(settings.analytics_retention_days))
}

/// Hash only source facts; AI results and report generation time cannot invalidate local metrics.
pub(crate) fn source_fingerprint(entry: &DictationHistoryEntry) -> AppResult<String> {
    hash_serialized(&(
        &entry.id,
        entry.created_at,
        entry.analytics_included,
        entry.analysis_status,
        &entry.original_text,
        &entry.analysis,
        entry
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.speech_timing),
    ))
}

fn hash_serialized(value: &impl serde::Serialize) -> AppResult<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
