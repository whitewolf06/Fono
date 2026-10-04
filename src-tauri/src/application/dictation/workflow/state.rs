//! Single pending owner and a small terminal cache make repeated actions safe.
use super::types::{PendingDictation, PendingPhase, PendingRequest};
use crate::{
    error::{AppError, AppResult},
    types::Transcript,
};

pub(super) struct Record<T> {
    pub snapshot: PendingDictation,
    pub data: T,
}
pub(super) struct Store<T> {
    pub pending: Option<Record<T>>,
    completed: Option<(u64, Option<Transcript>)>,
}
impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            pending: None,
            completed: None,
        }
    }
}
pub(super) enum Claim<T> {
    Work(T, PendingDictation),
    Completed(Option<Transcript>),
}

impl<T: Clone> Store<T> {
    pub fn install(&mut self, record: Record<T>) -> AppResult<()> {
        if self.pending.is_some() {
            return Err(AppError::Busy(
                "Сначала завершите предыдущую диктовку".into(),
            ));
        }
        self.pending = Some(record);
        Ok(())
    }
    pub fn claim(&mut self, request: &PendingRequest) -> AppResult<Claim<T>> {
        let Some(record) = self.pending.as_mut() else {
            return match &self.completed {
                Some((id, output)) if *id == request.session_id => {
                    Ok(Claim::Completed(output.clone()))
                }
                _ => Err(stale()),
            };
        };
        if record.snapshot.session_id != request.session_id {
            return Err(stale());
        }
        if record.snapshot.phase == PendingPhase::Processing {
            return Err(AppError::Busy("Диктовка уже обрабатывается".into()));
        }
        if record.snapshot.copy_only
            && matches!(
                request.action,
                super::PendingAction::InsertRaw | super::PendingAction::ProcessAndInsert
            )
        {
            return Err(AppError::Config(
                "Эта диктовка завершена для копирования. Скопируйте текст или закройте индикатор"
                    .into(),
            ));
        }
        if record.snapshot.insertion_blocked
            && !matches!(
                request.action,
                super::PendingAction::Complete | super::PendingAction::ProcessPreview
            )
        {
            return Err(AppError::Injection("Вставка заблокирована. Скопируйте текст из Fono; повторная отправка может дублировать текст".into()));
        }
        if let Some(preset) = request.preset {
            record.snapshot.preset = preset;
        }
        if let Some(language) = request.target_language {
            record.snapshot.target_language = language;
            record.snapshot.translation_enabled = language.is_some();
        }
        record.snapshot.phase = PendingPhase::Processing;
        record.snapshot.error = None;
        Ok(Claim::Work(record.data.clone(), record.snapshot.clone()))
    }
    pub fn retry(&mut self, id: u64, error: String, blocked: bool) -> bool {
        let Some(record) = self
            .pending
            .as_mut()
            .filter(|r| r.snapshot.session_id == id)
        else {
            return false;
        };
        record.snapshot.phase = PendingPhase::AwaitingAction;
        record.snapshot.error = Some(error);
        record.snapshot.insertion_blocked |= blocked;
        true
    }
    pub fn generated(&mut self, id: u64, text: String) -> bool {
        let Some(record) = self
            .pending
            .as_mut()
            .filter(|record| record.snapshot.session_id == id)
        else {
            return false;
        };
        record.snapshot.result_text = Some(text);
        true
    }
    #[cfg(test)]
    pub fn finish(&mut self, id: u64, output: Option<Transcript>) -> bool {
        self.detach(id, output).is_some()
    }
    // Runtime callers drop returned data after releasing the store mutex. Its
    // updater lease uses an outer admission lock and must never drop inside it.
    pub fn detach(&mut self, id: u64, output: Option<Transcript>) -> Option<Record<T>> {
        if self.pending.as_ref().map(|r| r.snapshot.session_id) != Some(id) {
            return None;
        }
        let record = self.pending.take();
        self.completed = Some((id, output));
        record
    }
    pub fn snapshot(&self) -> Option<PendingDictation> {
        self.pending.as_ref().map(|r| r.snapshot.clone())
    }
}
fn stale() -> AppError {
    AppError::Cancelled("Эта диктовка уже завершена или заменена".into())
}

#[cfg(test)]
mod tests;
