//! Stop and overlay choices share the capture fence and the session snapshot.
use super::Pipeline;
use crate::{
    error::{AppError, AppResult},
    types::Settings,
};
use fono_core::{OperationCancellation, OperationPhase, OperationSnapshot};
use std::sync::atomic::Ordering;

impl Pipeline {
    /// Restore a failed shortcut completion only while its exact capture is editable.
    pub(crate) fn while_editable_recording(&self, operation: u64, apply: impl FnOnce()) -> bool {
        let recording = self.recording.lock();
        if !*recording
            || self.processing_frozen_for.load(Ordering::Acquire) == operation
            || !self
                .current_operation()
                .is_some_and(|capture| capture.id == operation)
        {
            return false;
        }
        apply();
        true
    }

    /// Stop freezes the choice before cloning settings, even while the public
    /// phase still says Recording and audio drain/VAD have not completed yet.
    pub(crate) fn freeze_session_for_completion(
        &self,
        operation: u64,
        fallback: impl FnOnce() -> Settings,
    ) -> Option<(OperationSnapshot, OperationCancellation, Settings)> {
        let _capture = self.recording.lock();
        let current = self
            .current_operation()
            .filter(|item| item.id == operation)?;
        let cancellation = self.cancellation(operation)?;
        self.processing_frozen_for
            .store(operation, Ordering::Release);
        let settings = self
            .session_settings_for(operation)
            .unwrap_or_else(fallback);
        Some((current, cancellation, settings))
    }

    /// The synchronous callback validates pending ownership, persists its small
    /// settings delta, then updates snapshots. It must not reacquire this fence.
    pub(crate) fn with_processing_choice<T>(
        &self,
        operation: Option<u64>,
        apply: impl FnOnce(Option<OperationPhase>, Option<&mut Settings>) -> AppResult<T>,
    ) -> AppResult<T> {
        let recording = self.recording.lock();
        let current = self.current_operation();
        match operation {
            None if current.is_none() => apply(None, None),
            None => Err(AppError::Busy("Сначала завершите текущую диктовку".into())),
            Some(id) => {
                let current = current.filter(|item| item.id == id).ok_or_else(|| {
                    AppError::Cancelled("Эта диктовка уже завершена или заменена".into())
                })?;
                let editable = match current.phase {
                    OperationPhase::Recording => {
                        *recording && self.processing_frozen_for.load(Ordering::Acquire) != id
                    }
                    OperationPhase::AwaitingAction => true,
                    _ => false,
                };
                if !editable {
                    return Err(AppError::Busy(
                        "Выбор обработки уже зафиксирован. Дождитесь результата диктовки".into(),
                    ));
                }
                let mut settings = self.session_settings.lock();
                let session = settings
                    .as_mut()
                    .filter(|(session, _)| *session == id)
                    .map(|(_, settings)| settings)
                    .ok_or_else(|| {
                        AppError::Cancelled("Настройки этой диктовки недоступны".into())
                    })?;
                apply(Some(current.phase), Some(session))
            }
        }
    }
}

#[cfg(test)]
mod tests;
