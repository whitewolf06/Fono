//! Operation fencing and cleanup survive every early return and dropped future.
use crate::{
    error::{AppError, AppResult},
    operation::{OperationCancellation, OperationSource, TerminalReason},
    pipeline::{self, Pipeline},
    state::AppState,
    types::{PipelineState, Settings},
};
use tauri::{AppHandle, Manager};

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
        let current = pipeline
            .current_operation()
            .filter(|current| current.id == operation)
            .ok_or_else(|| AppError::Cancelled("Диктовка отменена или заменена".into()))?;
        let cancellation = pipeline.cancellation(operation).ok_or_else(|| {
            AppError::Internal("active dictation has no cancellation signal".into())
        })?;
        Ok(Self {
            app: app.clone(),
            operation,
            source: current.source,
            cancellation,
            settings: pipeline
                .session_settings_for(operation)
                .unwrap_or_else(|| app.state::<AppState>().settings()),
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
    }
}

pub(super) struct OperationScope {
    app: AppHandle,
    operation: u64,
    reason: TerminalReason,
}

impl OperationScope {
    pub fn new(app: AppHandle, operation: u64, pause_wake: bool) -> Self {
        if pause_wake {
            app.state::<fono_wake::WakeWordHandle>().pause();
        }
        Self {
            app,
            operation,
            reason: TerminalReason::Failed,
        }
    }

    pub fn complete<T>(&mut self, result: &AppResult<T>) {
        self.reason = match result {
            Ok(_) => TerminalReason::Completed,
            Err(AppError::Cancelled(_)) => TerminalReason::Cancelled,
            Err(_) => TerminalReason::Failed,
        };
    }
}

impl Drop for OperationScope {
    fn drop(&mut self) {
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
    if !crate::application::live_dictation::is_active(app) {
        app.state::<Pipeline>().while_idle(|| {
            app.state::<fono_wake::WakeWordHandle>().resume();
        });
    }
}

pub(super) fn emit_pipeline_error(app: &AppHandle, message: &str) {
    crate::events::emit_error(
        app,
        crate::events::ErrorCodeV1::Internal,
        message,
        app.state::<Pipeline>()
            .current_operation()
            .map(|item| item.id),
    );
}
