//! Shared capture, recognition and completion for ordinary and diagnostic dictation.
mod capture;
mod capture_activation;
mod diagnostic_sample;
mod diagnostics;
mod lifecycle;
mod postprocess;
mod processing_test;
mod recognition;
mod standard;
pub(crate) mod workflow;

#[allow(unused_imports)] // Preserve the existing application API.
pub(crate) use capture::start_command;
pub(crate) use capture::{ensure_capture_allowed, start, start_with_audio};
pub(crate) use capture::{start_command_operation, start_operation, start_with_audio_operation};
#[cfg(test)]
pub(crate) use diagnostic_sample::collect_samples;
pub(crate) use diagnostic_sample::record_diagnostic_sample;
pub(crate) use diagnostics::transcribe_test;
pub(crate) use lifecycle::resume_wake_if_idle;
pub(crate) use processing_test::{
    cancel as cancel_processing_test_capture, finish as finish_processing_test_capture,
    start as start_processing_test_capture, CaptureResult as ProcessingCaptureResult,
    CaptureStarted as ProcessingCaptureStarted, ProcessingCaptureRuntime,
};
pub(crate) use standard::{stop, stop_overlay_for, stop_with_reason, stop_with_reason_for};

use crate::{operation::OperationCancellation, types::Transcript};

pub(crate) async fn wait_for_cancellation(cancellation: OperationCancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn empty_transcript() -> Transcript {
    Transcript {
        text: String::new(),
        detected_language: None,
        transcribe_secs: None,
        audio_secs: None,
        device: None,
    }
}
