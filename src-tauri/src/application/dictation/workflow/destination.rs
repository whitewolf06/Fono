//! Captured field identity is scoped to one operation and one send attempt.
use crate::{
    error::{AppError, AppResult},
    injection::target::TextTarget,
};
use fono_core::OperationCancellation;

pub(super) const COPY_ONLY: &str =
    "Повторная вставка недоступна. Скопируйте текст из Fono и вставьте его в нужное поле";

struct Captured {
    operation: u64,
    target: Option<TextTarget>,
    claimed: bool,
    validity: OperationCancellation,
    context: Option<(u64, crate::types::ApplicationContext)>,
}

#[derive(Default)]
pub(super) struct DestinationSlot(Option<Captured>);

impl DestinationSlot {
    pub fn install(&mut self, operation: u64, target: Option<TextTarget>) {
        if let Some(previous) = self.0.take() {
            previous.validity.cancel();
        }
        self.0 = Some(Captured {
            operation,
            target,
            claimed: false,
            validity: OperationCancellation::default(),
            context: None,
        });
    }

    pub fn enable_context(&mut self, operation: u64, epoch: Option<u64>) {
        if let Some(record) = self.0.as_mut().filter(|r| r.operation == operation) {
            record.context = epoch.map(|e| (e, crate::types::ApplicationContext::default()));
        }
    }
    pub fn context(&self, operation: u64) -> Option<(u64, crate::types::ApplicationContext)> {
        self.0
            .as_ref()
            .filter(|r| r.operation == operation)
            .and_then(|r| r.context.clone())
    }
    pub fn observe(&mut self, operation: u64, context: crate::types::ApplicationContext) {
        if let Some((_, value)) = self
            .0
            .as_mut()
            .filter(|r| r.operation == operation)
            .and_then(|r| r.context.as_mut())
        {
            *value = context;
        }
    }

    pub fn clear(&mut self, operation: u64) {
        if self
            .0
            .as_ref()
            .is_some_and(|record| record.operation == operation)
        {
            if let Some(record) = self.0.take() {
                record.validity.cancel();
            }
        }
    }

    pub fn target(&self, operation: u64) -> Option<TextTarget> {
        self.0
            .as_ref()
            .filter(|record| record.operation == operation)
            .and_then(|record| record.target.clone())
    }

    pub fn claim(
        &mut self,
        operation: u64,
        target: &TextTarget,
    ) -> AppResult<OperationCancellation> {
        let record = self
            .0
            .as_mut()
            .filter(|record| {
                record.operation == operation
                    && !record.claimed
                    && !record.validity.is_cancelled()
                    && record.target.as_ref() == Some(target)
            })
            .ok_or_else(|| AppError::Injection(COPY_ONLY.into()))?;
        // Claim before dispatch, including failures/partial sends. There is no
        // acknowledgement from an arbitrary application that permits a replay.
        record.claimed = true;
        Ok(record.validity.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_and_observations_belong_only_to_the_captured_operation() {
        let mut s = DestinationSlot::default();
        s.install(1, Some(target()));
        assert!(s.context(1).is_none());
        s.enable_context(1, Some(9));
        s.observe(
            1,
            crate::types::ApplicationContext::unknown_attempt(
                crate::types::InjectionMode::Clipboard,
            ),
        );
        assert!(s.context(1).unwrap().1.attempted);
        s.install(2, Some(target()));
        s.enable_context(2, Some(10));
        s.observe(
            1,
            crate::types::ApplicationContext::unknown_attempt(
                crate::types::InjectionMode::Clipboard,
            ),
        );
        assert!(!s.context(2).unwrap().1.attempted);
        assert!(s.context(1).is_none());
    }

    fn target() -> TextTarget {
        TextTarget {
            window: 1,
            focus: 2,
            process: 3,
            runtime_id: vec![4, 5],
        }
    }

    #[test]
    fn claim_is_one_shot_even_after_a_partial_or_cancelled_send() {
        let mut slot = DestinationSlot::default();
        slot.install(11, Some(target()));
        let validity = slot.claim(11, &target()).unwrap();
        assert!(!validity.is_cancelled());
        assert!(slot.claim(11, &target()).is_err());
    }

    #[test]
    fn replacement_cancels_queued_old_send_even_for_the_same_target() {
        let mut slot = DestinationSlot::default();
        slot.install(11, Some(target()));
        let old = slot.claim(11, &target()).unwrap();
        slot.install(12, Some(target()));
        assert!(old.is_cancelled());
        assert!(slot.claim(11, &target()).is_err());
        assert!(slot.claim(12, &target()).is_ok());
    }

    #[test]
    fn stale_clear_cannot_invalidate_new_target() {
        let mut slot = DestinationSlot::default();
        slot.install(11, Some(target()));
        slot.install(12, Some(target()));
        slot.clear(11);
        let new = slot.claim(12, &target()).unwrap();
        assert!(!new.is_cancelled());
        slot.clear(12);
        assert!(new.is_cancelled());
    }

    #[test]
    fn ui_capture_and_a_different_field_never_authorize_input() {
        let mut slot = DestinationSlot::default();
        slot.install(11, None);
        assert!(slot.claim(11, &target()).is_err());
        slot.install(12, Some(target()));
        let different = TextTarget {
            runtime_id: vec![4, 6],
            ..target()
        };
        assert!(slot.claim(12, &different).is_err());
        assert!(slot.claim(12, &target()).is_ok());
    }
}
