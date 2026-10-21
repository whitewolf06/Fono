//! One owner finalizes a recording. Concurrent stop callers await that result.
use crate::{
    application::dictation::DictationOutcome,
    error::{AppError, AppResult},
};
use parking_lot::Mutex;
use tokio::sync::watch;

type ResultValue<T = DictationOutcome> = Option<Result<T, String>>;

#[derive(Default)]
pub struct CompletionGate(Mutex<Option<(u64, watch::Sender<ResultValue>)>>);

pub enum FinishClaim {
    Owner(FinishTicket),
    Waiting(watch::Receiver<ResultValue>),
}

pub struct FinishTicket(watch::Sender<ResultValue>);

impl super::Pipeline {
    pub(crate) fn claim_completion_for(&self, operation: u64) -> AppResult<Option<FinishClaim>> {
        if operation == 0 {
            return self.completion.claim(0).map(Some);
        }
        match self.while_operation(operation, || self.completion.claim(operation)) {
            Some(claim) => claim.map(Some),
            None => Ok(self
                .completion
                .existing(operation)
                .map(FinishClaim::Waiting)),
        }
    }
}

impl CompletionGate {
    /// A late request may observe an old result, but must not replace a
    /// successor's completion slot or become a new owner for a retired id.
    pub(crate) fn existing(&self, operation: u64) -> Option<watch::Receiver<ResultValue>> {
        self.0
            .lock()
            .as_ref()
            .filter(|(id, _)| *id == operation)
            .map(|(_, sender)| sender.subscribe())
    }
    /// A failed pre-Stop persistence barrier leaves capture editable. Current
    /// waiters receive that failure; a later explicit Stop can retry it.
    pub(crate) fn release_for_retry(&self, operation: u64) {
        let mut slot = self.0.lock();
        if slot.as_ref().is_some_and(|(id, _)| *id == operation) {
            slot.take();
        }
    }
    pub fn claim(&self, operation: u64) -> AppResult<FinishClaim> {
        let mut slot = self.0.lock();
        if let Some((id, sender)) = slot.as_ref() {
            if operation == *id || operation == 0 {
                return Ok(FinishClaim::Waiting(sender.subscribe()));
            }
        }
        if operation == 0 {
            return Err(AppError::Config("Нет активной диктовки".into()));
        }
        let (sender, _) = watch::channel(None);
        *slot = Some((operation, sender.clone()));
        Ok(FinishClaim::Owner(FinishTicket(sender)))
    }
}

impl FinishTicket {
    pub fn complete(self, result: &AppResult<DictationOutcome>) {
        self.0
            .send_replace(Some(result.as_ref().cloned().map_err(ToString::to_string)));
    }
}

impl Drop for FinishTicket {
    fn drop(&mut self) {
        if self.0.borrow().is_none() {
            self.0
                .send_replace(Some(Err("Завершение диктовки прервано".into())));
        }
    }
}

pub async fn wait<T: Clone>(mut receiver: watch::Receiver<ResultValue<T>>) -> AppResult<T> {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result.map_err(AppError::Internal);
        }
        receiver
            .changed()
            .await
            .map_err(|_| AppError::Cancelled("Диктовка закрыта".into()))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn duplicate_stop_receives_the_first_result() {
        let gate = CompletionGate::default();
        let FinishClaim::Owner(ticket) = gate.claim(4).unwrap() else {
            panic!()
        };
        let FinishClaim::Waiting(waiter) = gate.claim(4).unwrap() else {
            panic!()
        };
        ticket.complete(&Ok(DictationOutcome::Completed(crate::types::Transcript {
            text: "результат".into(),
            detected_language: None,
            transcribe_secs: None,
            audio_secs: None,
            device: None,
        })));
        assert!(
            matches!(wait(waiter).await.unwrap(), DictationOutcome::Completed(t) if t.text == "результат")
        );
        let FinishClaim::Waiting(waiter) = gate.claim(0).unwrap() else {
            panic!()
        };
        assert!(
            matches!(wait(waiter).await.unwrap(), DictationOutcome::Completed(t) if t.text == "результат")
        );
    }
    #[tokio::test]
    async fn dropped_owner_does_not_leave_waiters_hanging() {
        let gate = CompletionGate::default();
        let FinishClaim::Owner(ticket) = gate.claim(1).unwrap() else {
            panic!()
        };
        let FinishClaim::Waiting(waiter) = gate.claim(1).unwrap() else {
            panic!()
        };
        drop(ticket);
        assert!(wait(waiter).await.is_err());
    }

    #[tokio::test]
    async fn duplicate_stop_preserves_terminal_and_deferred_outcomes() {
        for outcome in [
            DictationOutcome::NoSpeech,
            DictationOutcome::Cancelled,
            DictationOutcome::Superseded,
            DictationOutcome::Deferred(crate::types::Transcript {
                text: "ожидает решения".into(),
                detected_language: None,
                transcribe_secs: None,
                audio_secs: None,
                device: None,
            }),
        ] {
            let gate = CompletionGate::default();
            let FinishClaim::Owner(ticket) = gate.claim(4).unwrap() else {
                panic!()
            };
            let FinishClaim::Waiting(waiter) = gate.claim(4).unwrap() else {
                panic!()
            };
            let expected = serde_json::to_value(&outcome).unwrap();
            ticket.complete(&Ok(outcome));
            assert_eq!(
                serde_json::to_value(wait(waiter).await.unwrap()).unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn failed_pre_stop_barrier_is_shared_but_a_later_explicit_stop_can_retry() {
        let gate = CompletionGate::default();
        let FinishClaim::Owner(first) = gate.claim(4).unwrap() else {
            panic!()
        };
        let FinishClaim::Waiting(waiter) = gate.claim(4).unwrap() else {
            panic!()
        };
        gate.release_for_retry(4);
        first.complete(&Err(AppError::Config("save failed".into())));
        assert!(wait(waiter).await.is_err());
        let FinishClaim::Owner(retry) = gate.claim(4).unwrap() else {
            panic!()
        };
        retry.complete(&Ok(DictationOutcome::NoSpeech));
        assert!(matches!(
            wait(gate.existing(4).unwrap()).await.unwrap(),
            DictationOutcome::NoSpeech
        ));
    }
}
