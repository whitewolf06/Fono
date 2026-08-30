//! Durable local history for completed REST transcription jobs.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;

use crate::application::transcription_jobs::{JobState, TranscriptionJob};
use crate::error::AppResult;

const MAX_HISTORY_ENTRIES: usize = 100;
static HISTORY: Lazy<ServiceHistoryRepository> = Lazy::new(ServiceHistoryRepository::default);

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceHistorySnapshot {
    pub jobs: Vec<TranscriptionJob>,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub total_audio_seconds: f32,
    pub total_transcribe_seconds: f32,
}

#[derive(Default)]
pub struct ServiceHistoryRepository {
    write_lock: Mutex<()>,
}

impl ServiceHistoryRepository {
    pub fn list(&self) -> AppResult<Vec<TranscriptionJob>> {
        let _guard = self.write_lock.lock();
        crate::state::load_transcription_history_document()
    }

    pub fn upsert_terminal(&self, job: TranscriptionJob) -> AppResult<()> {
        if !job.is_terminal() {
            return Ok(());
        }
        let _guard = self.write_lock.lock();
        let mut jobs: Vec<TranscriptionJob> = crate::state::load_transcription_history_document()?;
        jobs.retain(|entry| entry.id != job.id);
        jobs.insert(0, job);
        jobs.sort_by_key(|entry| std::cmp::Reverse(entry.created_at_ms));
        jobs.truncate(MAX_HISTORY_ENTRIES);
        crate::state::save_transcription_history_document(&jobs)
    }

    pub fn clear(&self) -> AppResult<()> {
        let _guard = self.write_lock.lock();
        crate::state::save_transcription_history_document::<TranscriptionJob>(&[])
    }
}

pub fn upsert_terminal(job: TranscriptionJob) -> AppResult<()> {
    HISTORY.upsert_terminal(job)
}

pub fn clear() -> AppResult<()> {
    HISTORY.clear()
}

pub fn snapshot() -> AppResult<ServiceHistorySnapshot> {
    let jobs = HISTORY.list()?;
    let mut snapshot = ServiceHistorySnapshot {
        jobs,
        ..ServiceHistorySnapshot::default()
    };
    for job in &snapshot.jobs {
        match job.state {
            JobState::Completed => snapshot.completed += 1,
            JobState::Failed => snapshot.failed += 1,
            JobState::Cancelled => snapshot.cancelled += 1,
            _ => {}
        }
        if let Some(result) = &job.result {
            snapshot.total_audio_seconds += result.audio_seconds.unwrap_or_default();
            snapshot.total_transcribe_seconds += result.transcribe_seconds.unwrap_or_default();
        }
    }
    Ok(snapshot)
}
