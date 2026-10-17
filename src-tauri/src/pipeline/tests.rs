use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

use crate::audio::{AudioRecorder, RecordingWriter};
use crate::error::{AppError, AppResult};
use crate::types::PipelineState;
use fono_core::{
    OperationEvent, OperationPhase, OperationResource, OperationSource, TerminalReason,
};

use super::{append_bounded, Pipeline};

#[test]
fn desktop_state_is_always_the_current_coordinator_projection() {
    let pipeline = Pipeline::new();
    assert_eq!(pipeline.state(), PipelineState::Idle);
    let old = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    assert_eq!(pipeline.state(), PipelineState::Listening);
    for (phase, state) in [
        (OperationPhase::Transcribing, PipelineState::Transcribing),
        (OperationPhase::Processing, PipelineState::Processing),
        (
            OperationPhase::AwaitingAction,
            PipelineState::AwaitingAction,
        ),
        (OperationPhase::Injecting, PipelineState::Injecting),
    ] {
        pipeline.operations.transition(old.id, phase).unwrap();
        assert_eq!(pipeline.state(), state);
    }
    pipeline.cancel_for(old.id).unwrap();
    assert_eq!(pipeline.state(), PipelineState::Idle);
    let next = pipeline.operations.start(OperationSource::Ui).unwrap();
    assert!(pipeline
        .sync_operation_state_for(old.id, PipelineState::Idle, TerminalReason::Completed)
        .is_none());
    assert_eq!(pipeline.state(), PipelineState::Listening);
    assert_eq!(pipeline.current_operation().unwrap().id, next.id);
}

#[test]
fn inactive_result_distinguishes_cancellation_from_replacement() {
    use crate::application::dictation::DictationOutcome;
    let pipeline = Pipeline::new();
    let old = pipeline.operations.start(OperationSource::Ui).unwrap();
    pipeline.cancel_for(old.id).unwrap();
    assert!(matches!(
        DictationOutcome::interrupted(&pipeline, old.id),
        DictationOutcome::Cancelled
    ));
    let next = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    assert!(matches!(
        DictationOutcome::interrupted(&pipeline, old.id),
        DictationOutcome::Superseded
    ));
    pipeline
        .finish_operation(next.id, TerminalReason::Completed)
        .unwrap();
    assert!(matches!(
        DictationOutcome::interrupted(&pipeline, old.id),
        DictationOutcome::Superseded
    ));
    assert!(matches!(
        DictationOutcome::interrupted(&pipeline, 0),
        DictationOutcome::Cancelled
    ));
}

#[test]
fn cancellation_before_terminal_claim_wins_the_explicit_result() {
    use crate::application::dictation::DictationOutcome;
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    pipeline.cancel_for(operation.id).unwrap();
    let mut published = false;
    let (outcome, retained) = pipeline.settle_dictation_outcome(
        operation.id,
        DictationOutcome::NoSpeech,
        || false,
        |_| published = true,
    );
    assert!(matches!(outcome, DictationOutcome::Cancelled));
    assert!(!retained);
    assert!(!published);
}

#[test]
fn terminal_claim_before_cancellation_cannot_be_reversed() {
    use crate::application::dictation::DictationOutcome;
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    let mut published = None;
    let (outcome, retained) = pipeline.settle_dictation_outcome(
        operation.id,
        DictationOutcome::NoSpeech,
        || false,
        |event| published = Some(event),
    );
    assert!(matches!(outcome, DictationOutcome::NoSpeech));
    assert!(!retained);
    assert!(
        matches!(published, Some(OperationEvent::Finished(terminal)) if terminal.reason == TerminalReason::Completed)
    );
    assert!(pipeline.cancel_for(operation.id).is_none());
    assert_eq!(pipeline.state(), PipelineState::Idle);
}

#[test]
fn deferred_claim_keeps_operation_until_the_owner_decides() {
    use crate::application::dictation::DictationOutcome;
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    pipeline
        .operations
        .transition(operation.id, OperationPhase::Transcribing)
        .unwrap();
    pipeline
        .operations
        .transition(operation.id, OperationPhase::AwaitingAction)
        .unwrap();
    let transcript = crate::types::Transcript {
        text: "на решение".into(),
        detected_language: None,
        transcribe_secs: None,
        audio_secs: None,
        device: None,
    };
    let (outcome, retained) = pipeline.settle_dictation_outcome(
        operation.id,
        DictationOutcome::Deferred(transcript),
        || true,
        |_| panic!("must not finish deferred operation"),
    );
    assert!(matches!(outcome, DictationOutcome::Deferred(_)));
    assert!(retained);
    assert_eq!(pipeline.state(), PipelineState::AwaitingAction);
    assert!(pipeline.is_operation_active(operation.id));
    assert_eq!(
        pipeline.operations.resources(operation.id),
        Some(Vec::new())
    );
    assert!(pipeline.cancel_for(operation.id).is_some());
}

#[test]
fn durable_commit_and_terminal_ownership_precede_a_queued_cancel() {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    let mut durable = false;
    let mut terminal = false;
    let result = pipeline.commit_terminal(
        operation.id,
        || {
            durable = true;
            7
        },
        |_| terminal = true,
    );
    assert_eq!(result, Some(7));
    assert!(durable && terminal);
    assert!(pipeline.cancel_for(operation.id).is_none());
    assert_eq!(pipeline.state(), PipelineState::Idle);
    let next = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    assert!(pipeline
        .commit_terminal(operation.id, || panic!("late archive must not run"), |_| {})
        .is_none());
    assert_eq!(pipeline.current_operation().unwrap().id, next.id);
}

#[test]
fn cancel_before_durable_claim_blocks_both_archive_and_terminal_publication() {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    pipeline.cancel_for(operation.id).unwrap();
    assert!(pipeline
        .commit_terminal(
            operation.id,
            || panic!("cancelled archive must not run"),
            |_| panic!("terminal already published")
        )
        .is_none());
}

struct FailingAudioRecorder;

impl AudioRecorder<RecordingWriter, AppError> for FailingAudioRecorder {
    fn start(
        &self,
        _device_id: Option<&str>,
        _writer: RecordingWriter,
        _limit_reached: Arc<AtomicBool>,
        _level_bits: Arc<AtomicU32>,
        _maximum_samples: usize,
    ) -> AppResult<()> {
        Err(AppError::Audio("injected audio acquisition failure".into()))
    }

    fn stop(&self) -> AppResult<()> {
        Ok(())
    }

    fn shutdown(&self) {}
}

#[test]
fn cancellation_invalidates_in_flight_operation() {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();
    assert!(pipeline.is_operation_active(operation.id));

    pipeline.cancel();
    assert!(!pipeline.is_operation_active(operation.id));

    let next_operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    assert!(pipeline.is_operation_active(next_operation.id));
    assert_ne!(operation.id, next_operation.id);
}

#[test]
fn audio_acquisition_failure_releases_operation_and_resource_lease() {
    let pipeline = Pipeline::new_with_audio_owner(Box::new(FailingAudioRecorder));

    let error = pipeline
        .start_recording_from(None, OperationSource::WakeWord)
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("injected audio acquisition failure"));
    assert_eq!(pipeline.operation_id(), 0);
    assert!(!pipeline.is_recording());
}

#[test]
fn shutdown_is_idempotent_and_finishes_the_active_operation() {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Ui).unwrap();

    assert!(matches!(
        pipeline.shutdown(),
        Some(OperationEvent::Finished(terminal))
            if terminal.id == operation.id && terminal.reason == TerminalReason::Cancelled
    ));
    assert!(pipeline.shutdown().is_none());
    assert!(!pipeline.is_operation_active(operation.id));
}

#[test]
fn service_lifecycle_does_not_block_interactive_recording() {
    let pipeline = Pipeline::new();

    let (operation_id, cancellation) = pipeline.start_service_transcription().unwrap();

    assert_eq!(pipeline.operation_id(), 0);
    assert!(!cancellation.is_cancelled());
    assert_eq!(
        pipeline.service_operations.resources(operation_id),
        Some(vec![OperationResource::Stt])
    );
    let interactive = pipeline.operations.start(OperationSource::Ui).unwrap();
    pipeline.finish_service_operation(operation_id, TerminalReason::Completed);
    assert!(pipeline.is_operation_active(interactive.id));
    assert!(pipeline.service_operations.current().is_none());
}

#[test]
fn pipeline_state_updates_are_bound_to_the_active_operation() {
    let pipeline = Pipeline::new();
    let operation = pipeline
        .operations
        .start(OperationSource::WakeWord)
        .unwrap();

    assert!(matches!(
        pipeline.sync_operation_state(PipelineState::Transcribing),
        Some(OperationEvent::PhaseChanged(snapshot))
            if snapshot.id == operation.id && snapshot.phase == OperationPhase::Transcribing
    ));
    assert!(pipeline
        .sync_operation_state(PipelineState::Processing)
        .is_some());
    assert!(matches!(
        pipeline.sync_operation_state(PipelineState::Idle),
        Some(OperationEvent::Finished(terminal))
            if terminal.id == operation.id && terminal.reason == TerminalReason::Completed
    ));
    assert!(!pipeline.is_operation_active(operation.id));
}

#[test]
fn awaiting_action_releases_stt_but_keeps_operation_and_retries() {
    let pipeline = Pipeline::new();
    let operation = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    pipeline
        .sync_operation_state_for(
            operation.id,
            PipelineState::Transcribing,
            TerminalReason::Completed,
        )
        .unwrap();
    pipeline
        .sync_operation_state_for(
            operation.id,
            PipelineState::AwaitingAction,
            TerminalReason::Completed,
        )
        .unwrap();
    assert!(pipeline.is_operation_active(operation.id));
    assert!(!pipeline
        .operations
        .resources(operation.id)
        .unwrap()
        .contains(&OperationResource::Stt));
    pipeline
        .sync_operation_state_for(
            operation.id,
            PipelineState::Processing,
            TerminalReason::Completed,
        )
        .unwrap();
    pipeline
        .sync_operation_state_for(
            operation.id,
            PipelineState::AwaitingAction,
            TerminalReason::Completed,
        )
        .unwrap();
    pipeline
        .sync_operation_state_for(operation.id, PipelineState::Idle, TerminalReason::Cancelled)
        .unwrap();
    assert!(!pipeline.is_operation_active(operation.id));
}

#[test]
fn bounded_recording_never_exceeds_its_sample_limit() {
    let mut samples = vec![1, 2];

    assert!(append_bounded(&mut samples, &[3, 4, 5], 4));
    assert_eq!(samples, vec![1, 2, 3, 4]);
    assert!(append_bounded(&mut samples, &[6], 4));
    assert_eq!(samples.len(), 4);
}

#[test]
fn bounded_recording_accepts_a_complete_chunk_when_capacity_is_available() {
    let mut samples = Vec::new();

    assert!(!append_bounded(&mut samples, &[1, 2, 3], 4));
    assert_eq!(samples, vec![1, 2, 3]);
}
