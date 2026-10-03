//! One owner finalizes a recording. Concurrent stop callers await that result.
use crate::{
    error::{AppError, AppResult},
    types::Transcript,
};
use parking_lot::Mutex;
use tokio::sync::watch;

type ResultValue = Option<Result<Transcript, String>>;

#[derive(Default)]
pub struct CompletionGate(Mutex<Option<(u64, watch::Sender<ResultValue>)>>);

pub enum FinishClaim {
    Owner(FinishTicket),
    Waiting(watch::Receiver<ResultValue>),
}

pub struct FinishTicket(watch::Sender<ResultValue>);

impl CompletionGate {
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
    pub fn complete(self, result: &AppResult<Transcript>) {
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

pub async fn wait(mut receiver: watch::Receiver<ResultValue>) -> AppResult<Transcript> {
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
        ticket.complete(&Ok(Transcript {
            text: "результат".into(),
            detected_language: None,
            transcribe_secs: None,
            audio_secs: None,
            device: None,
        }));
        assert_eq!(wait(waiter).await.unwrap().text, "результат");
        let FinishClaim::Waiting(waiter) = gate.claim(0).unwrap() else {
            panic!()
        };
        assert_eq!(wait(waiter).await.unwrap().text, "результат");
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
}
