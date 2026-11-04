//! One bounded recording, one attempt owner, and operation-scoped retirement.
use super::{RecoveryPhase, RecoveryReason, RecoverySnapshot};
use crate::{
    application::dictation::DictationOutcome,
    error::{AppError, AppResult},
};

pub(super) struct Record<T> {
    pub snapshot: RecoverySnapshot,
    pub data: T,
    in_flight: bool,
}
pub(super) struct Store<T> {
    pub record: Option<Record<T>>,
    completed: Option<(u64, DictationOutcome)>,
}
impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            record: None,
            completed: None,
        }
    }
}
impl<T: Clone> Store<T> {
    pub fn install(&mut self, snapshot: RecoverySnapshot, data: T) -> AppResult<()> {
        if self.record.is_some() {
            return Err(AppError::Busy("Запись уже ожидает распознавания".into()));
        }
        let in_flight = snapshot.phase == RecoveryPhase::Recognizing;
        self.record = Some(Record {
            snapshot,
            data,
            in_flight,
        });
        Ok(())
    }
    pub fn claim(&mut self, operation: u64) -> AppResult<Result<T, DictationOutcome>> {
        let Some(record) = self
            .record
            .as_mut()
            .filter(|record| record.snapshot.session_id == operation)
        else {
            return match &self.completed {
                Some((id, outcome)) if *id == operation => Ok(Err(outcome.clone())),
                _ => Err(AppError::Cancelled("Диктовка отменена или заменена".into())),
            };
        };
        if record.in_flight || record.snapshot.phase != RecoveryPhase::AwaitingAction {
            return Err(AppError::Busy(
                "Распознавание записи уже выполняется".into(),
            ));
        }
        record.snapshot.phase = RecoveryPhase::Recognizing;
        record.in_flight = true;
        Ok(Ok(record.data.clone()))
    }
    pub fn retry(&mut self, operation: u64, reason: RecoveryReason) -> bool {
        let Some(record) = self
            .record
            .as_mut()
            .filter(|record| record.snapshot.session_id == operation)
        else {
            return false;
        };
        record.snapshot.phase = RecoveryPhase::AwaitingAction;
        record.snapshot.reason = reason;
        true
    }
    pub fn release(&mut self, operation: u64) {
        if let Some(record) = self
            .record
            .as_mut()
            .filter(|record| record.snapshot.session_id == operation)
        {
            record.in_flight = false;
        }
    }
    pub fn detach(&mut self, operation: u64, outcome: Option<DictationOutcome>) -> Option<T> {
        if !self
            .record
            .as_ref()
            .is_some_and(|record| record.snapshot.session_id == operation)
        {
            return None;
        }
        let record = self.record.take().expect("matching recovery");
        if let Some(outcome) = outcome {
            self.completed = Some((operation, outcome));
        }
        Some(record.data)
    }
    pub fn complete(&mut self, operation: u64, outcome: DictationOutcome) {
        // A late result may not overwrite a successor's cancellation/result.
        if self
            .completed
            .as_ref()
            .map_or(true, |(id, _)| *id < operation)
        {
            self.completed = Some((operation, outcome));
        }
    }
    pub fn completed(&self, operation: u64) -> Option<DictationOutcome> {
        self.completed
            .as_ref()
            .filter(|(id, _)| *id == operation)
            .map(|(_, outcome)| outcome.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn snapshot(id: u64) -> RecoverySnapshot {
        RecoverySnapshot {
            session_id: id,
            phase: RecoveryPhase::AwaitingAction,
            reason: RecoveryReason::ServerFailed,
            audio_secs: 2.0,
            local_available: false,
        }
    }
    #[test]
    fn retry_keeps_exact_pcm_and_cannot_claim_twice() {
        let pcm = Arc::new(vec![1_i16, -1, 42]);
        let mut store = Store::default();
        store.install(snapshot(7), pcm.clone()).unwrap();
        let retained = store.claim(7).unwrap().unwrap();
        assert!(Arc::ptr_eq(&pcm, &retained));
        assert!(matches!(store.claim(7), Err(AppError::Busy(_))));
        assert!(store.retry(7, RecoveryReason::LocalFailed));
        assert!(matches!(store.claim(7), Err(AppError::Busy(_))));
        store.release(7);
        assert!(Arc::ptr_eq(&pcm, &store.claim(7).unwrap().unwrap()));
    }
    #[test]
    fn cancellation_releases_owned_recording_and_old_retry_cannot_reappear() {
        struct Lease(Arc<AtomicUsize>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        store
            .install(snapshot(8), Arc::new(Lease(dropped.clone())))
            .unwrap();
        let physical_worker = store.claim(8).unwrap().unwrap();
        drop(store.detach(8, Some(DictationOutcome::Cancelled)));
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        assert!(!store.retry(8, RecoveryReason::ServerFailed));
        assert!(matches!(
            store.claim(8).unwrap(),
            Err(DictationOutcome::Cancelled)
        ));
        store
            .install(snapshot(9), Arc::new(Lease(dropped.clone())))
            .unwrap();
        assert!(!store.retry(8, RecoveryReason::ServerFailed));
        store.complete(8, DictationOutcome::NoSpeech);
        assert_eq!(store.record.as_ref().unwrap().snapshot.session_id, 9);
        drop(physical_worker);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn completion_is_idempotent_and_stale_actions_do_not_consume_new_audio() {
        let mut store = Store::default();
        store.install(snapshot(1), Arc::new(vec![1_i16])).unwrap();
        store.detach(1, None);
        store.complete(1, DictationOutcome::NoSpeech);
        assert!(matches!(
            store.claim(1).unwrap(),
            Err(DictationOutcome::NoSpeech)
        ));
        store.install(snapshot(2), Arc::new(vec![2_i16])).unwrap();
        assert!(store.detach(1, Some(DictationOutcome::Cancelled)).is_none());
        assert_eq!(store.record.as_ref().unwrap().data.as_slice(), &[2_i16]);
    }
}
