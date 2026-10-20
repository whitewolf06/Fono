//! A validated prefix and a disjoint tail produce one ordinary Transcript.
use crate::{
    application::{segmented_transcription::append_text, transcription_context},
    error::{AppError, AppResult},
    pipeline::capture_segments::{CaptureSegmentBatch, CaptureSegmentProfile},
    types::{Settings, Transcript},
};
use fono_audio::speech_segments::{
    SpeechSegment, StreamingSpeechSegmenter, DEFAULT_MAX_SEGMENT_SAMPLES,
};
use fono_core::OperationCancellation;
use fono_stt_protocol::{BackendKind, WindowTranscript};
use std::path::Path;

/// The same blocking Stop implementation is used by desktop and public-WAV replay.
pub(super) struct Runtime<'a> {
    pub stt: &'a crate::stt::SttEngine,
    pub scheduler: &'a std::sync::Arc<crate::pipeline::scheduler::SttScheduler>,
}

impl Runtime<'_> {
    pub fn finish(
        &self,
        operation: u64,
        settings: &Settings,
        samples: &[i16],
        batch: CaptureSegmentBatch,
        cancelled: &OperationCancellation,
        paths: impl FnOnce() -> AppResult<(std::path::PathBuf, crate::stt::WorkerPaths)>,
    ) -> AppResult<Transcript> {
        check_cancel(cancelled)?;
        if batch.processed_until == samples.len() {
            return complete(
                operation,
                settings,
                samples,
                batch,
                &[],
                cancelled,
                |_, _, _| unreachable!("no remaining PCM"),
            );
        }
        let (vad_path, workers) = paths()?;
        let ranges = tail_ranges(samples, batch.processed_until, &vad_path, cancelled)?;
        let mut gate = fono_audio::StreamingSpeechVad::new(&vad_path)
            .map_err(|error| AppError::Stt(error.to_string()))?;
        let model = settings
            .selected_stt_model()
            .ok_or(AppError::ModelNotSelected)?;
        complete(
            operation,
            settings,
            samples,
            batch,
            &ranges,
            cancelled,
            |window, start, context| {
                // A short forced-cut fragment needs whole-capture fallback, not loss.
                if window.len() < fono_audio::speech_vad::MIN_RECORDING_SAMPLES {
                    return Err(AppError::Config(
                        "short tail requires complete-capture fallback".into(),
                    ));
                }
                let speech = gate
                    .analyze_recording(window, || cancelled.is_cancelled())
                    .ok_or_else(|| {
                        AppError::Cancelled("tail speech validation cancelled".into())
                    })?;
                if !speech.has_speech {
                    return Ok(None);
                }
                let permit = self.scheduler.acquire(true, cancelled)?;
                self.stt.ensure_model_loaded(
                    &model,
                    settings.acceleration,
                    &workers,
                    &permit.cancellation,
                )?;
                let context =
                    context.filter(|_| self.stt.capabilities().is_some_and(|c| c.initial_prompt));
                self.stt
                    .transcribe_window_with_options(
                        window,
                        &settings.language,
                        context,
                        operation,
                        &permit.cancellation,
                        start,
                        settings.decoding_options(),
                    )
                    .map(Some)
            },
        )
    }
}

pub(super) fn validate_prefix(
    operation: u64,
    settings: &Settings,
    length: usize,
    batch: &CaptureSegmentBatch,
) -> AppResult<()> {
    if batch.operation_id != operation
        || batch.profile != CaptureSegmentProfile::from(settings)
        || batch.results.len() > 512
    {
        return Err(AppError::Config(
            "capture prefix identity/profile mismatch".into(),
        ));
    }
    let mut expected = 0;
    let mut bytes = 0;
    for (index, result) in batch.results.iter().enumerate() {
        if result.operation_id != operation
            || result.index != index
            || result.range.start != expected
            || result.range.end <= expected
            || result.range.end > length
        {
            return Err(AppError::Config(
                "capture prefix ranges are inconsistent".into(),
            ));
        }
        if let Some(transcript) = &result.transcript {
            if !transcript.transcribe_secs.is_finite() || transcript.transcribe_secs < 0.0 {
                return Err(AppError::Config(
                    "capture prefix duration is invalid".into(),
                ));
            }
            bytes += transcript.text.len()
                + transcript.detected_language.as_ref().map_or(0, String::len);
        }
        expected = result.range.end;
    }
    if batch.processed_until != expected || bytes > 256 * 1024 {
        return Err(AppError::Config(
            "capture prefix checkpoint is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn tail_ranges(
    samples: &[i16],
    from: usize,
    model: &Path,
    cancelled: &OperationCancellation,
) -> AppResult<Vec<SpeechSegment>> {
    check_cancel(cancelled)?;
    let tail = samples
        .get(from..)
        .ok_or_else(|| AppError::Config("tail starts past capture".into()))?;
    let mut ranges = Vec::new();
    if tail.is_empty() {
        return Ok(ranges);
    }
    let mut planner = StreamingSpeechSegmenter::new(model, DEFAULT_MAX_SEGMENT_SAMPLES)
        .map_err(|e| AppError::Stt(e.to_string()))?;
    let mut emit = |segment: fono_audio::speech_segments::ClosedSpeechSegment| {
        ranges.push(SpeechSegment {
            start: segment.range.start + from,
            end: segment.range.end + from,
            forced: segment.range.forced,
        });
    };
    for chunk in tail.chunks(512) {
        check_cancel(cancelled)?;
        planner
            .accept(chunk, &mut emit)
            .map_err(|e| AppError::Stt(e.to_string()))?;
    }
    check_cancel(cancelled)?;
    planner
        .finish(&mut emit)
        .map_err(|e| AppError::Stt(e.to_string()))?;
    Ok(ranges)
}

fn check_cancel(cancelled: &OperationCancellation) -> AppResult<()> {
    if cancelled.is_cancelled() {
        Err(AppError::Cancelled("segment completion cancelled".into()))
    } else {
        Ok(())
    }
}

pub(super) fn complete(
    operation: u64,
    settings: &Settings,
    samples: &[i16],
    batch: CaptureSegmentBatch,
    ranges: &[SpeechSegment],
    cancelled: &OperationCancellation,
    mut recognize: impl FnMut(&[i16], u64, Option<&str>) -> AppResult<Option<WindowTranscript>>,
) -> AppResult<Transcript> {
    check_cancel(cancelled)?;
    validate_prefix(operation, settings, samples.len(), &batch)?;
    let mut expected = batch.processed_until;
    for range in ranges {
        if range.start != expected || range.end <= expected || range.end > samples.len() {
            return Err(AppError::Config(
                "tail must partition the remaining PCM".into(),
            ));
        }
        expected = range.end;
    }
    if expected != samples.len() {
        return Err(AppError::Config("tail segmentation lost PCM".into()));
    }
    let mut combined = Combined::new(samples.len());
    for result in batch.results {
        check_cancel(cancelled)?;
        combined.append(result.transcript)?;
    }
    for range in ranges {
        check_cancel(cancelled)?;
        let result = recognize(
            &samples[range.start..range.end],
            range.start as u64,
            transcription_context::bounded(&combined.context),
        )?;
        check_cancel(cancelled)?;
        combined.append(result)?;
    }
    check_cancel(cancelled)?;
    Ok(combined.finish())
}

struct Combined {
    transcript: Transcript,
    backend: Option<BackendKind>,
    context: String,
}

impl Combined {
    fn new(length: usize) -> Self {
        Self {
            transcript: Transcript {
                text: String::new(),
                detected_language: None,
                transcribe_secs: Some(0.0),
                audio_secs: Some(length as f32 / 16_000.0),
                device: None,
            },
            backend: None,
            context: String::new(),
        }
    }
    fn append(&mut self, result: Option<WindowTranscript>) -> AppResult<()> {
        let Some(result) = result else {
            return Ok(());
        };
        if self
            .backend
            .is_some_and(|backend| backend != result.backend)
        {
            return Err(AppError::Stt(
                "recognizer device changed during capture completion".into(),
            ));
        }
        if !result.transcribe_secs.is_finite() || result.transcribe_secs < 0.0 {
            return Err(AppError::Stt("recognizer duration is invalid".into()));
        }
        self.backend = Some(result.backend);
        if let Some(context) = transcription_context::bounded(&result.text) {
            self.context = context.to_owned();
        }
        append_text(&mut self.transcript.text, &result.text);
        self.transcript.transcribe_secs =
            Some(self.transcript.transcribe_secs.unwrap_or(0.0) + result.transcribe_secs);
        if self.transcript.detected_language.is_none() {
            self.transcript.detected_language = result.detected_language;
        }
        Ok(())
    }
    fn finish(mut self) -> Transcript {
        self.transcript.device = self.backend.map(|backend| {
            match backend {
                BackendKind::Cpu => "CPU",
                BackendKind::Cuda => "CUDA",
                BackendKind::Vulkan => "Vulkan",
            }
            .into()
        });
        if self.backend.is_none() {
            self.transcript.transcribe_secs = None;
        }
        self.transcript
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, windows))]
mod quality;
