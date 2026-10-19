//! Operation ownership and projection from the authoritative coordinator.
use super::*;

impl Pipeline {
    /// Запросить отмену текущей диктовки.
    pub fn cancel(&self) -> Option<OperationEvent> {
        let operation = self.operations.current()?;
        self.cancel_for(operation.id)
    }

    /// Delayed callbacks can cancel only the operation they captured earlier.
    pub fn cancel_for(&self, operation: u64) -> Option<OperationEvent> {
        let _capture = self.recording.lock();
        let event = self.operations.cancel(operation);
        if event.is_some() {
            #[cfg(feature = "speech-vad")]
            self.capture_segments.stop(operation);
            self.scheduler.release_reservation(operation);
            tracing::debug!(operation, "pipeline: cancellation requested");
        }
        event
    }

    pub fn shutdown(&self) -> Option<OperationEvent> {
        self.stt.stop_recovery();
        let event = self.cancel();
        let _ = self.stop_recording();
        self.audio_owner.shutdown();
        event
    }

    pub fn finish_operation(
        &self,
        operation_id: u64,
        reason: TerminalReason,
    ) -> Option<OperationEvent> {
        #[cfg(feature = "speech-vad")]
        self.capture_segments.stop(operation_id);
        self.scheduler.release_reservation(operation_id);
        self.operations.finish(operation_id, reason)
    }

    pub fn operation_id(&self) -> u64 {
        self.operations
            .current()
            .map_or(0, |operation| operation.id)
    }

    pub fn current_operation(&self) -> Option<OperationSnapshot> {
        self.operations.current()
    }

    /// UI state is a projection of the coordinator, never independently stored.
    pub fn state(&self) -> PipelineState {
        Self::project_state(self.current_operation().as_ref())
    }

    pub(crate) fn project_state(operation: Option<&OperationSnapshot>) -> PipelineState {
        match operation.map(|operation| operation.phase) {
            None => PipelineState::Idle,
            Some(OperationPhase::Recording) => PipelineState::Listening,
            Some(OperationPhase::Transcribing) => PipelineState::Transcribing,
            Some(OperationPhase::AwaitingAction) => PipelineState::AwaitingAction,
            Some(OperationPhase::Processing) => PipelineState::Processing,
            Some(OperationPhase::Injecting) => PipelineState::Injecting,
        }
    }
    pub fn has_active_service_operation(&self) -> bool {
        self.service_operations.current().is_some()
    }

    pub fn is_operation_active(&self, operation_id: u64) -> bool {
        self.operations.is_active(operation_id)
    }

    pub(crate) fn is_operation_superseded(&self, operation: u64) -> bool {
        operation != 0 && self.operations.latest_operation_id() != operation
    }

    /// Cancellation and terminal success claim the same capture fence. Once a
    /// result wins this claim, a delayed Cancel cannot retire it a second time.
    pub(crate) fn settle_dictation_outcome(
        &self,
        operation: u64,
        outcome: crate::application::dictation::DictationOutcome,
        retains: impl FnOnce() -> bool,
        publish: impl FnOnce(OperationEvent),
    ) -> (crate::application::dictation::DictationOutcome, bool) {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return (
                crate::application::dictation::DictationOutcome::interrupted(self, operation),
                false,
            );
        }
        if retains() {
            return (outcome, true);
        }
        if let Some(event) = self.finish_operation(operation, outcome.terminal_reason()) {
            publish(event);
        }
        (outcome, false)
    }

    /// The durable effect and terminal ownership have one linearization point.
    /// The callback may persist state but must not reacquire the capture fence.
    pub(crate) fn commit_terminal<T>(
        &self,
        operation: u64,
        commit: impl FnOnce() -> T,
        publish: impl FnOnce(OperationEvent),
    ) -> Option<T> {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return None;
        }
        let result = commit();
        if let Some(event) = self.finish_operation(operation, TerminalReason::Completed) {
            publish(event);
        }
        Some(result)
    }

    /// Commit an in-process side effect before cancellation or replacement.
    /// The closure must not reacquire this lock, run inference or send native input.
    pub(crate) fn while_operation<T>(
        &self,
        operation: u64,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return None;
        }
        Some(action())
    }

    /// Idle cleanup and wake resumption complete before a new capture can attach.
    pub(crate) fn while_idle(&self, action: impl FnOnce()) -> bool {
        let _capture = self.recording.lock();
        if self.current_operation().is_some() {
            return false;
        }
        action();
        true
    }

    pub fn cancellation(&self, operation_id: u64) -> Option<OperationCancellation> {
        self.operations.cancellation(operation_id)
    }

    /// Service lifecycle is independent from interactive dictation. The STT
    /// scheduler pauses a service at a bounded window when dictation starts.
    pub fn start_service_transcription(&self) -> AppResult<(u64, OperationCancellation)> {
        let _configuration_admission = crate::application::capture_configuration::begin_capture()?;
        let _update_admission = crate::application::updates::activity::begin()?;
        let operation = self.service_operations.start(OperationSource::Service)?;
        if let Err(error) = self
            .service_operations
            .acquire_resource(operation.id, OperationResource::Stt)
        {
            let _ = self
                .service_operations
                .finish(operation.id, TerminalReason::Failed);
            return Err(error.into());
        }
        let _ = self
            .service_operations
            .transition(operation.id, OperationPhase::Transcribing);
        let cancellation = self
            .service_operations
            .cancellation(operation.id)
            .expect("new service operation must own cancellation");
        Ok((operation.id, cancellation))
    }

    /// Подтвердить текущую диктовку (закончить запись досрочно).
    pub fn confirm(&self) {
        if let Some(operation) = self.operations.current() {
            self.confirm_for(operation.id);
        }
    }

    pub fn is_operation_confirmed(&self, operation_id: u64) -> bool {
        self.operations
            .current()
            .is_some_and(|operation| operation.id == operation_id && operation.confirmed)
    }

    pub fn sync_operation_state(&self, state: PipelineState) -> Option<OperationEvent> {
        let operation = self.operations.current()?;
        self.sync_operation_state_for(operation.id, state, TerminalReason::Completed)
    }

    pub fn sync_operation_state_for(
        &self,
        operation_id: u64,
        state: PipelineState,
        terminal_reason: TerminalReason,
    ) -> Option<OperationEvent> {
        let event = match state {
            PipelineState::Idle => self.operations.finish(operation_id, terminal_reason),
            PipelineState::Error => self.operations.finish(operation_id, TerminalReason::Failed),
            PipelineState::Listening => self
                .operations
                .transition(operation_id, OperationPhase::Recording),
            PipelineState::Transcribing => self
                .operations
                .transition(operation_id, OperationPhase::Transcribing),
            PipelineState::Processing => self
                .operations
                .transition(operation_id, OperationPhase::Processing),
            PipelineState::AwaitingAction => self
                .operations
                .transition(operation_id, OperationPhase::AwaitingAction),
            PipelineState::Injecting => self
                .operations
                .transition(operation_id, OperationPhase::Injecting),
        };

        if event.is_some() {
            if matches!(state, PipelineState::Idle | PipelineState::Error) {
                #[cfg(feature = "speech-vad")]
                self.capture_segments.stop(operation_id);
                self.scheduler.release_reservation(operation_id);
            }
            if matches!(
                state,
                PipelineState::Processing | PipelineState::AwaitingAction
            ) {
                self.scheduler.release_reservation(operation_id);
            }
        }
        event
    }

    pub fn finish_service_operation(&self, operation: u64, reason: TerminalReason) {
        let _ = self.service_operations.finish(operation, reason);
    }
}
