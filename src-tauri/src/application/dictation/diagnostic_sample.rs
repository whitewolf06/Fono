//! Microphone and wake setup share a bounded, cancellable capture lifecycle.
use super::{capture::ensure_capture_allowed, lifecycle::OperationScope, wait_for_cancellation};
use crate::{
    error::{AppError, AppResult},
    operation::{OperationSource, TerminalReason},
    pipeline::{self, Pipeline},
    state::AppState,
    types::PipelineState,
};
use tauri::{AppHandle, Manager};

pub(crate) async fn record_diagnostic_sample(
    app: &AppHandle,
    device_id: Option<&str>,
    duration: std::time::Duration,
) -> AppResult<Vec<i16>> {
    ensure_capture_allowed(app)?;
    let pipeline = app.state::<Pipeline>();
    let operation = pipeline.start_recording_from(device_id, OperationSource::Diagnostics)?;
    let mut scope = OperationScope::new(app.clone(), operation, true);
    let result = collect(app, operation, duration).await;
    scope.complete(&result);
    result
}

async fn collect(
    app: &AppHandle,
    operation: u64,
    duration: std::time::Duration,
) -> AppResult<Vec<i16>> {
    let pipeline = app.state::<Pipeline>();
    let cancellation = pipeline
        .cancellation(operation)
        .ok_or_else(|| AppError::Cancelled("Проверка микрофона отменена".into()))?;
    if !pipeline::set_state_for_operation(
        app,
        app.state::<AppState>().inner(),
        &pipeline,
        operation,
        PipelineState::Listening,
        TerminalReason::Completed,
    ) {
        return Err(AppError::Cancelled("Проверка микрофона отменена".into()));
    }
    collect_samples(&pipeline, operation, duration, cancellation).await
}

pub(crate) async fn collect_samples(
    pipeline: &Pipeline,
    operation: u64,
    duration: std::time::Duration,
    cancellation: crate::operation::OperationCancellation,
) -> AppResult<Vec<i16>> {
    tokio::select! {
        _ = tokio::time::sleep(duration) => {},
        _ = wait_for_cancellation(cancellation) => {
            return Err(AppError::Cancelled("Проверка микрофона отменена".into()));
        }
    }
    if !pipeline.is_operation_active(operation) {
        return Err(AppError::Cancelled("Проверка микрофона отменена".into()));
    }
    let samples = pipeline
        .stop_recording_for(operation)?
        .ok_or_else(|| AppError::Cancelled("Проверка микрофона отменена".into()))?;
    if samples.is_empty() {
        return Err(AppError::Audio(
            "Тестовая запись пуста: проверьте микрофон".into(),
        ));
    }
    Ok(samples)
}
