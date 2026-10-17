//! Publish coordinator projections while preserving capture and UI dispatch fences.
use super::*;

/// Caller owns the capture fence and has already retired this exact operation.
pub(crate) fn publish_terminal_claim(handle: &AppHandle, event: OperationEvent) {
    crate::events::emit_operation(handle, event);
    crate::events::emit_pipeline_state(handle, PipelineState::Idle);
    meter::sync(handle, PipelineState::Idle);
}

pub(crate) fn settle_outcome(
    handle: &AppHandle,
    pipeline: &Pipeline,
    operation: u64,
    outcome: crate::application::dictation::DictationOutcome,
) -> (crate::application::dictation::DictationOutcome, bool) {
    let result = pipeline.settle_dictation_outcome(
        operation,
        outcome,
        || crate::application::dictation::workflow::retains(handle, operation),
        |event| {
            crate::events::emit_operation(handle, event);
            crate::events::emit_pipeline_state(handle, PipelineState::Idle);
            meter::sync(handle, PipelineState::Idle);
        },
    );
    if !result.1 {
        sync_overlay_window(handle, pipeline.state());
    }
    result
}

/// Обновляет состояние FSM, эмитит событие во фронтенд и управляет overlay-окном.
pub fn set_state(handle: &AppHandle, _state: &AppState, new: PipelineState) {
    let pipeline = handle.state::<Pipeline>();
    let capture = pipeline.recording.lock();
    if let Some(event) = pipeline.sync_operation_state(new) {
        crate::events::emit_operation(handle, event);
    }
    let projected = pipeline.state();
    tracing::debug!("pipeline state -> {projected:?}");
    crate::events::emit_pipeline_state(handle, projected);
    meter::sync(handle, projected);
    drop(capture);
    sync_overlay_window(handle, projected);
}

/// Applies a state transition only if it belongs to the specified operation.
/// This prevents an old async STT/LLM result from hiding the overlay or
/// finishing a newer dictation after cancellation.
pub fn set_state_for_operation(
    handle: &AppHandle,
    _state: &AppState,
    pipeline: &Pipeline,
    operation_id: u64,
    new: PipelineState,
    terminal_reason: TerminalReason,
) -> bool {
    let _capture = pipeline.recording.lock();
    let Some(event) = pipeline.sync_operation_state_for(operation_id, new, terminal_reason) else {
        tracing::debug!(
            operation = operation_id,
            ?new,
            "ignoring stale pipeline state update"
        );
        return false;
    };
    crate::events::emit_operation(handle, event);
    let projected = pipeline.state();
    tracing::debug!(operation = operation_id, "pipeline state -> {projected:?}");
    crate::events::emit_pipeline_state(handle, projected);
    meter::sync(handle, projected);
    // Publish under the capture fence, but never hold it across window dispatch.
    drop(_capture);
    sync_overlay_window(handle, projected);
    true
}

/// Cancellation already emitted its terminal event; publish Idle only while vacant.
pub(crate) fn set_idle_if_no_operation(
    handle: &AppHandle,
    _state: &AppState,
    pipeline: &Pipeline,
) -> bool {
    let committed = pipeline.while_idle(|| {
        crate::events::emit_pipeline_state(handle, PipelineState::Idle);
        meter::sync(handle, PipelineState::Idle);
    });
    if committed {
        sync_overlay_window(handle, PipelineState::Idle);
    }
    committed
}

pub(crate) fn sync_overlay_window(handle: &AppHandle, _state: PipelineState) {
    // Visibility is recomputed when the UI task executes. A queued old Idle
    // notification must not hide the overlay of a newer recording.
    crate::overlay::request_visibility(handle);
}
