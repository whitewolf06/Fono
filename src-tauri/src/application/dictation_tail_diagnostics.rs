//! Privacy-safe measurements for investigating a missing dictation tail.
//!
//! The diagnostic is deliberately an ephemeral structured log record. It keeps
//! sample counts and stage outcomes only: never audio samples, transcript text,
//! model paths, or error messages that could contain dictated text.

use crate::operation::OperationSource;
use crate::vad::VadResult;

const SAMPLE_RATE_HZ: usize = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DictationStopReason {
    Manual,
    WakeSilence,
    WakeConfirmed,
    WakeTimeout,
}

impl DictationStopReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::WakeSilence => "wake_silence",
            Self::WakeConfirmed => "wake_confirmed",
            Self::WakeTimeout => "wake_timeout",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SttOutcome {
    NotStarted,
    Succeeded,
    Empty,
    Failed,
    Cancelled,
}

impl SttOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Succeeded => "succeeded",
            Self::Empty => "empty",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostprocessorOutcome {
    NotReached,
    Skipped,
    Unchanged,
    Changed,
    Fallback,
    Cancelled,
}

impl PostprocessorOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotReached => "not_reached",
            Self::Skipped => "skipped",
            Self::Unchanged => "unchanged",
            Self::Changed => "changed",
            Self::Fallback => "fallback",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstSuspectedLayer {
    Capture,
    Vad,
    Stt,
    Postprocessor,
}

impl FirstSuspectedLayer {
    fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Vad => "vad",
            Self::Stt => "stt",
            Self::Postprocessor => "postprocessor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VadMeasurement {
    applied: bool,
    detected_speech: bool,
    input_samples: usize,
    output_samples: usize,
    trailing_before_samples: usize,
    trailing_after_samples: usize,
}

/// Builds one record for a completed dictation flow.
///
/// The caller only passes numeric lengths and boolean outcomes. In particular,
/// it must not pass transcript text or a slice of microphone samples here.
#[derive(Debug)]
pub(crate) struct DictationTailDiagnostic {
    operation: u64,
    source: OperationSource,
    stop_reason: DictationStopReason,
    captured_samples: Option<usize>,
    vad: Option<VadMeasurement>,
    stt: SttOutcome,
    postprocessor: PostprocessorOutcome,
    emitted: bool,
}

impl DictationTailDiagnostic {
    pub(crate) fn new(
        operation: u64,
        source: OperationSource,
        stop_reason: DictationStopReason,
    ) -> Self {
        Self {
            operation,
            source,
            stop_reason,
            captured_samples: None,
            vad: None,
            stt: SttOutcome::NotStarted,
            postprocessor: PostprocessorOutcome::NotReached,
            emitted: false,
        }
    }

    pub(crate) fn record_capture(&mut self, samples: usize) {
        self.captured_samples = Some(samples);
    }
    pub(crate) fn captured_samples(&self) -> Option<usize> {
        self.captured_samples
    }

    pub(crate) fn record_vad(
        &mut self,
        input_samples: usize,
        output_samples: usize,
        vad: &VadResult,
        retained_trailing_samples: usize,
    ) {
        let applied = vad.has_speech && vad.end_sample > vad.start_sample;
        self.vad = Some(VadMeasurement {
            applied,
            detected_speech: vad.has_speech,
            input_samples,
            output_samples,
            trailing_before_samples: input_samples.saturating_sub(vad.speech_end_sample),
            // The caller supplies the numeric tail in the exact buffer passed
            // to STT. A leading-only trim intentionally retains more than
            // VadResult::end_sample's right padding.
            trailing_after_samples: retained_trailing_samples.min(output_samples),
        });
    }

    pub(crate) fn skip_vad(&mut self, captured_samples: usize) {
        self.vad = Some(VadMeasurement {
            applied: false,
            detected_speech: false,
            input_samples: captured_samples,
            output_samples: captured_samples,
            trailing_before_samples: 0,
            trailing_after_samples: 0,
        });
    }

    pub(crate) fn stt_succeeded(&mut self, has_text: bool) {
        self.stt = if has_text {
            SttOutcome::Succeeded
        } else {
            SttOutcome::Empty
        };
    }

    pub(crate) fn stt_failed(&mut self) {
        self.stt = SttOutcome::Failed;
    }

    pub(crate) fn stt_cancelled(&mut self) {
        self.stt = SttOutcome::Cancelled;
    }

    pub(crate) fn postprocessor_skipped(&mut self) {
        self.postprocessor = PostprocessorOutcome::Skipped;
    }

    pub(crate) fn postprocessor_succeeded(&mut self, changed_text: bool) {
        self.postprocessor = if changed_text {
            PostprocessorOutcome::Changed
        } else {
            PostprocessorOutcome::Unchanged
        };
    }

    pub(crate) fn postprocessor_fallback(&mut self) {
        self.postprocessor = PostprocessorOutcome::Fallback;
    }

    pub(crate) fn postprocessor_cancelled(&mut self) {
        self.postprocessor = PostprocessorOutcome::Cancelled;
    }

    fn first_suspected_layer(&self) -> Option<FirstSuspectedLayer> {
        if self.captured_samples.map_or(true, |samples| samples == 0) {
            return Some(FirstSuspectedLayer::Capture);
        }
        if self
            .vad
            .is_some_and(|vad| vad.applied && vad.output_samples == 0)
        {
            return Some(FirstSuspectedLayer::Vad);
        }
        if matches!(self.stt, SttOutcome::Empty | SttOutcome::Failed) {
            return Some(FirstSuspectedLayer::Stt);
        }
        if matches!(self.postprocessor, PostprocessorOutcome::Changed) {
            return Some(FirstSuspectedLayer::Postprocessor);
        }
        None
    }

    /// Writes one stable, content-free record that developers can compare over
    /// the hotkey and wake-word scenario matrix.
    pub(crate) fn emit(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        let captured_samples = self.captured_samples.unwrap_or_default();
        let vad = self.vad.unwrap_or(VadMeasurement {
            applied: false,
            detected_speech: false,
            input_samples: captured_samples,
            output_samples: captured_samples,
            trailing_before_samples: 0,
            trailing_after_samples: 0,
        });
        let suspected = self
            .first_suspected_layer()
            .map(FirstSuspectedLayer::as_str)
            .unwrap_or("none");

        tracing::info!(
            event = "dictation_tail_diagnostic",
            operation = self.operation,
            source = source_label(self.source),
            stop_reason = self.stop_reason.as_str(),
            captured_samples,
            captured_ms = samples_to_ms(captured_samples),
            vad_applied = vad.applied,
            vad_detected_speech = vad.detected_speech,
            vad_input_samples = vad.input_samples,
            vad_output_samples = vad.output_samples,
            vad_trailing_before_ms = samples_to_ms(vad.trailing_before_samples),
            vad_trailing_after_ms = samples_to_ms(vad.trailing_after_samples),
            stt = self.stt.as_str(),
            postprocessor = self.postprocessor.as_str(),
            first_suspected_layer = suspected,
            "dictation tail diagnostic completed"
        );
    }
}

impl Drop for DictationTailDiagnostic {
    fn drop(&mut self) {
        self.emit();
    }
}

fn source_label(source: OperationSource) -> &'static str {
    match source {
        OperationSource::Ui => "ui",
        OperationSource::Hotkey => "hotkey",
        OperationSource::WakeWord => "wake_word",
        OperationSource::Diagnostics => "diagnostics",
        OperationSource::Service => "service",
    }
}

fn samples_to_ms(samples: usize) -> u64 {
    (samples as u64 * 1_000) / SAMPLE_RATE_HZ as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech_vad() -> VadResult {
        VadResult {
            start_sample: 1_600,
            end_sample: 8_000,
            speech_end_sample: 5_600,
            has_speech: true,
            trimmed_samples: 3_200,
        }
    }

    #[test]
    fn records_vad_tail_lengths_without_audio_content() {
        let mut diagnostic =
            DictationTailDiagnostic::new(7, OperationSource::Hotkey, DictationStopReason::Manual);
        diagnostic.record_capture(9_600);
        diagnostic.record_vad(9_600, 8_000, &speech_vad(), 4_000);
        diagnostic.stt_succeeded(true);
        diagnostic.postprocessor_skipped();

        let vad = diagnostic.vad.expect("VAD measurement");
        assert_eq!(vad.trailing_before_samples, 4_000);
        assert_eq!(vad.trailing_after_samples, 4_000);
        assert_eq!(diagnostic.first_suspected_layer(), None);
    }

    #[test]
    fn classifies_empty_capture_before_later_stages() {
        let diagnostic = DictationTailDiagnostic::new(
            8,
            OperationSource::WakeWord,
            DictationStopReason::WakeSilence,
        );

        assert_eq!(
            diagnostic.first_suspected_layer(),
            Some(FirstSuspectedLayer::Capture)
        );
    }

    #[test]
    fn classifies_changed_postprocessor_only_after_successful_stt() {
        let mut diagnostic =
            DictationTailDiagnostic::new(9, OperationSource::Hotkey, DictationStopReason::Manual);
        diagnostic.record_capture(16_000);
        diagnostic.skip_vad(16_000);
        diagnostic.stt_succeeded(true);
        diagnostic.postprocessor_succeeded(true);

        assert_eq!(
            diagnostic.first_suspected_layer(),
            Some(FirstSuspectedLayer::Postprocessor)
        );
    }
}
