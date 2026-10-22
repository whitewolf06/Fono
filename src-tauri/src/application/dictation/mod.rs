//! Shared capture, recognition and completion for ordinary and diagnostic dictation.
mod auto_stop;
mod capture;
mod capture_activation;
#[cfg(feature = "speech-vad")]
mod capture_segments;
mod diagnostic_sample;
mod diagnostics;
mod lifecycle;
mod outcome;
mod postprocess;
mod processing_test;
mod recognition;
#[cfg(feature = "speech-vad")]
mod segment_completion;
mod standard;
pub(crate) mod wake_sample;
pub(crate) mod workflow;

#[cfg(feature = "wake-word")]
pub(crate) use capture::{ensure_capture_allowed, start_with_audio_operation};
pub(crate) use capture::{reserve_operation, start_reserved_operation};
pub(crate) use capture::{start, start_command_operation, start_operation, start_with_audio};
#[cfg(test)]
pub(crate) use diagnostic_sample::collect_samples;
pub(crate) use diagnostic_sample::record_diagnostic_sample;
pub(crate) use diagnostics::transcribe_test;
pub(crate) use lifecycle::resume_wake_if_idle;
pub(crate) use lifecycle::SettlementReceipt;
pub use outcome::{DictationOutcome, DictationOutcomeKind};
pub(crate) use processing_test::{
    cancel as cancel_processing_test_capture, finish as finish_processing_test_capture,
    start as start_processing_test_capture, CaptureResult as ProcessingCaptureResult,
    CaptureStarted as ProcessingCaptureStarted, ProcessingCaptureRuntime,
};
pub(crate) use standard::stop_after_overlay_flush_for;
pub(crate) use standard::{stop, stop_overlay_for, stop_with_reason, stop_with_reason_for};

use fono_core::OperationCancellation;

pub(crate) async fn wait_for_cancellation(cancellation: OperationCancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
