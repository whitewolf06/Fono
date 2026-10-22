//! Operation fencing and cleanup survive every early return and dropped future.
use crate::{
    error::{AppError, AppResult},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{PipelineState, Settings},
};
use fono_core::{OperationCancellation, OperationSource, TerminalReason};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};

#[derive(Clone)]
pub(crate) struct SettlementReceipt {
    operation: u64,
    committed: Arc<AtomicBool>,
}

impl SettlementReceipt {
    pub(crate) fn commit(&self, operation: u64) {
        assert_eq!(operation, self.operation);
        self.committed.store(true, Ordering::Release);
    }
    fn is_committed(&self) -> bool {
        self.committed.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub(super) struct Session {
    pub app: AppHandle,
    pub operation: u64,
    pub source: OperationSource,
    pub settings: Settings,
    pub cancellation: OperationCancellation,
}

impl Session {
    pub fn current(app: &AppHandle, operation: u64) -> AppResult<Self> {
        let pipeline = app.state::<Pipeline>();
        let (current, cancellation, settings) = pipeline
            .freeze_session_for_completion(operation, || {
                app.state::<AppState>().dictation_settings()
            })
            .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()))?;
        Ok(Self {
            app: app.clone(),
            operation,
            source: current.source,
            cancellation,
            settings,
        })
    }

    pub fn active(&self, context: &str) -> bool {
        let active = self
            .app
            .state::<Pipeline>()
            .is_operation_active(self.operation);
        if !active {
            tracing::info!(
                operation = self.operation,
                context,
                "stale dictation result discarded"
            );
        }
        active
    }

    pub fn interrupted(&self) -> super::DictationOutcome {
        super::DictationOutcome::interrupted(&self.app.state::<Pipeline>(), self.operation)
    }

    pub fn transition(&self, new: PipelineState, reason: TerminalReason) -> bool {
        pipeline::set_state_for_operation(
            &self.app,
            self.app.state::<AppState>().inner(),
            &self.app.state::<Pipeline>(),
            self.operation,
            new,
            reason,
        )
    }

    pub fn error(&self, code: crate::events::ErrorCodeV1, message: impl Into<String>) {
        if !self.active("before error publication") {
            return;
        }
        crate::events::emit_error(&self.app, code, message.into(), Some(self.operation));
        crate::application::recording_sounds::error(&self.app, self.operation, self.source);
    }
}

pub(super) struct OperationScope {
    app: AppHandle,
    operation: u64,
    reason: TerminalReason,
    settled: bool,
    receipt: SettlementReceipt,
}

impl OperationScope {
    pub fn new(app: AppHandle, operation: u64, pause_wake: bool) -> Self {
        if pause_wake {
            crate::application::wake_runtime::pause(&app);
        }
        Self {
            app,
            operation,
            reason: TerminalReason::Failed,
            settled: false,
            receipt: SettlementReceipt {
                operation,
                committed: Arc::new(AtomicBool::new(false)),
            },
        }
    }

    pub fn settlement_receipt(&self) -> SettlementReceipt {
        self.receipt.clone()
    }

    pub fn complete<T>(&mut self, result: &AppResult<T>) {
        self.reason = match result {
            Ok(_) => TerminalReason::Completed,
            Err(AppError::Cancelled(_)) => TerminalReason::Cancelled,
            Err(_) => TerminalReason::Failed,
        };
    }

    pub fn complete_outcome(&mut self, result: &mut AppResult<super::DictationOutcome>) {
        self.reason = match result {
            Ok(outcome) => outcome.terminal_reason(),
            Err(AppError::Cancelled(_)) => TerminalReason::Cancelled,
            Err(_) => TerminalReason::Failed,
        };
        let Ok(outcome) = result else {
            return;
        };
        if self.receipt.is_committed() {
            self.settled = true;
            super::workflow::clear_target(&self.app, self.operation);
            resume_wake_if_idle(&self.app);
            return;
        }
        let pipeline = self.app.state::<Pipeline>();
        if let Err(error) = pipeline.stop_recording_for(self.operation) {
            tracing::warn!(operation = self.operation, %error, "dictation settlement could not stop capture");
        }
        let (settled, retained) =
            pipeline::settle_outcome(&self.app, &pipeline, self.operation, outcome.clone());
        *outcome = settled;
        self.reason = outcome.terminal_reason();
        self.settled = true;
        if !retained {
            super::workflow::clear_target(&self.app, self.operation);
            resume_wake_if_idle(&self.app);
        }
    }
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        if self.settled || self.receipt.is_committed() {
            return;
        }
        // Stop is complete, but a deferred action still owns this operation.
        if super::workflow::retains(&self.app, self.operation) {
            return;
        }
        super::workflow::clear_target(&self.app, self.operation);
        let pipeline = self.app.state::<Pipeline>();
        if self.reason != TerminalReason::Completed {
            if let Some(cancellation) = pipeline.cancellation(self.operation) {
                cancellation.cancel();
            }
        }
        if let Err(error) = pipeline.stop_recording_for(self.operation) {
            tracing::warn!(operation = self.operation, %error, "dictation cleanup could not stop capture");
        }
        if pipeline
            .current_operation()
            .is_some_and(|current| current.id == self.operation)
        {
            pipeline::set_state_for_operation(
                &self.app,
                self.app.state::<AppState>().inner(),
                &pipeline,
                self.operation,
                PipelineState::Idle,
                self.reason,
            );
        }
        // A cancelled old diagnostic must never resume wake over a new capture.
        resume_wake_if_idle(&self.app);
    }
}

pub(crate) fn resume_wake_if_idle(app: &AppHandle) {
    #[cfg(feature = "experimental-live")]
    if crate::application::live_dictation::is_active(app) {
        return;
    }
    app.state::<Pipeline>().while_idle(|| {
        crate::application::wake_runtime::resume(app);
    });
}

pub(super) fn emit_pipeline_error(app: &AppHandle, message: &str) {
    if let Some(operation) = app.state::<Pipeline>().current_operation() {
        crate::application::recording_sounds::error(app, operation.id, operation.source);
    }
    crate::events::emit_error(
        app,
        crate::events::ErrorCodeV1::Internal,
        message,
        app.state::<Pipeline>()
            .current_operation()
            .map(|item| item.id),
    );
}
