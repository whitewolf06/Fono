//! One bounded capture consumer owns segmentation, scheduling and checkpoints.
//! No PCM queue: while STT runs, audio stays in the existing bounded writer.
use super::scheduler::SttScheduler;
use crate::{
    application::updates::activity::{self, ActivityLease},
    audio::RecordingWriter,
    error::{AppError, AppResult},
};
use fono_audio::{
    speech_segments::{SpeechSegment, StreamingSpeechSegmenter, DEFAULT_MAX_SEGMENT_SAMPLES},
    StreamingSpeechVad,
};
use fono_core::OperationCancellation;
use fono_stt_protocol::WindowTranscript;
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_TASKS: usize = 2;
const MAX_RESULTS: usize = 512;
const MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSegmentProfile {
    model: Option<crate::types::SttModelSelection>,
    acceleration: crate::types::AccelerationMode,
    language: String,
    decoding: fono_stt_protocol::DecodingOptions,
}

impl From<&crate::types::Settings> for CaptureSegmentProfile {
    fn from(settings: &crate::types::Settings) -> Self {
        Self {
            model: settings.selected_stt_model(),
            acceleration: settings.acceleration,
            language: settings.language.clone(),
            decoding: settings.decoding_options(),
        }
    }
}

pub struct CaptureSegmentResult {
    pub operation_id: u64,
    pub index: usize,
    pub range: SpeechSegment,
    /// None is a completed NoSpeech range, never an implicit STT failure.
    pub transcript: Option<WindowTranscript>,
}

pub struct CaptureSegmentBatch {
    pub operation_id: u64,
    pub profile: CaptureSegmentProfile,
    pub results: Vec<CaptureSegmentResult>,
    /// Everything from this offset onward still belongs to Stop.
    pub processed_until: usize,
}

pub struct CaptureSegmentTask {
    operation: u64,
    cancellation: OperationCancellation,
    stop: OperationCancellation,
    completed: Arc<AtomicUsize>,
    handle: Option<JoinHandle<AppResult<CaptureSegmentBatch>>>,
}

impl CaptureSegmentTask {
    pub fn stop(&self) {
        self.stop.cancel();
    }

    pub fn completed_segments(&self) -> usize {
        self.completed.load(Ordering::Acquire)
    }

    /// Blocking join belongs in spawn_blocking. A cancelled/dropped waiter
    /// cannot release the worker's activity lease or its inference permit.
    pub fn finish(mut self) -> AppResult<CaptureSegmentBatch> {
        self.stop();
        let result = self
            .handle
            .take()
            .expect("capture worker joined once")
            .join()
            .map_err(|_| AppError::Internal("capture segment worker failed".into()))?;
        if self.cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "capture segment operation cancelled".into(),
            ));
        }
        result
    }
}

impl Drop for CaptureSegmentTask {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
pub(super) struct CaptureSegments(Mutex<BTreeMap<u64, CaptureSegmentTask>>, Arc<AtomicUsize>);

struct WorkerSlot(Arc<AtomicUsize>);

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CaptureSegments {
    pub(super) fn start(
        &self,
        input: CaptureSegmentInput,
        recognize: impl FnMut(&[i16], u64, Option<&str>, &OperationCancellation) -> AppResult<WindowTranscript>
            + Send
            + 'static,
        activity: ActivityLease<'static>,
    ) -> AppResult<()> {
        let mut tasks = self.0.lock();
        if tasks.contains_key(&input.operation) {
            return Err(AppError::Busy(
                "capture segment worker already registered".into(),
            ));
        }
        tasks.retain(|_, task| !task.handle.as_ref().is_some_and(JoinHandle::is_finished));
        if self.1.load(Ordering::Acquire) >= MAX_TASKS {
            return Err(AppError::Busy(
                "capture segment worker capacity reached".into(),
            ));
        }
        self.1.fetch_add(1, Ordering::AcqRel);
        let slot = WorkerSlot(self.1.clone());
        let operation = input.operation;
        let cancellation = input.cancellation.clone();
        let stop = OperationCancellation::default();
        let completed = Arc::new(AtomicUsize::new(0));
        let worker_completed = completed.clone();
        let worker_stop = stop.clone();
        let handle = thread::Builder::new()
            .name("fono-capture-stt".into())
            .spawn(move || {
                let _activity = activity;
                let _slot = slot;
                run(input, worker_stop, recognize, worker_completed)
            })
            .map_err(|error| AppError::Internal(format!("capture segment start: {error}")))?;
        tasks.insert(
            operation,
            CaptureSegmentTask {
                operation,
                cancellation,
                stop,
                completed,
                handle: Some(handle),
            },
        );
        Ok(())
    }

    pub(super) fn stop(&self, operation: u64) {
        if let Some(task) = self.0.lock().get(&operation) {
            task.stop();
        }
    }

    pub(super) fn take(&self, operation: u64) -> Option<CaptureSegmentTask> {
        self.0.lock().remove(&operation)
    }
}

pub(super) struct CaptureSegmentInput {
    pub operation: u64,
    pub profile: CaptureSegmentProfile,
    pub cancellation: OperationCancellation,
    pub writer: RecordingWriter,
    pub scheduler: Arc<SttScheduler>,
    pub vad_path: PathBuf,
    pub start_after_samples: usize,
}

fn run(
    input: CaptureSegmentInput,
    stop: OperationCancellation,
    mut recognize: impl FnMut(
        &[i16],
        u64,
        Option<&str>,
        &OperationCancellation,
    ) -> AppResult<WindowTranscript>,
    completed: Arc<AtomicUsize>,
) -> AppResult<CaptureSegmentBatch> {
    let mut batch = CaptureSegmentBatch {
        operation_id: input.operation,
        profile: input.profile.clone(),
        results: Vec::new(),
        processed_until: 0,
    };
    let cancelled = input.cancellation.combined_with(&stop);
    let mut segmenter = StreamingSpeechSegmenter::new(&input.vad_path, DEFAULT_MAX_SEGMENT_SAMPLES)
        .map_err(|e| AppError::Stt(e.to_string()))?;
    let mut gate =
        StreamingSpeechVad::new(&input.vad_path).map_err(|e| AppError::Stt(e.to_string()))?;
    let mut cursor = 0;
    let mut text_bytes = 0;
    let mut context = String::new();
    loop {
        if input.cancellation.is_cancelled() {
            return Err(AppError::Cancelled("capture cancelled".into()));
        }
        if stop.is_cancelled() {
            return Ok(batch);
        }
        let (start, samples) = {
            let writer = input.writer.lock();
            if writer.end() < input.start_after_samples as u64 {
                (cursor, Vec::new())
            } else {
                writer.window(cursor, 512)
            }
        };
        // Stop drains/takes the writer under the capture fence. It may win
        // between the loop guard and this snapshot; that is not a cursor fault.
        if input.cancellation.is_cancelled() {
            return Err(AppError::Cancelled("capture cancelled".into()));
        }
        if stop.is_cancelled() {
            return Ok(batch);
        }
        if start != cursor {
            return Err(AppError::Audio("capture segment cursor lost".into()));
        }
        if samples.is_empty() {
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        cursor += samples.len() as u64;
        let mut closed = None;
        segmenter
            .accept(&samples, |segment| closed = Some(segment))
            .map_err(|e| AppError::Stt(e.to_string()))?;
        let Some(segment) = closed else {
            continue;
        };
        if batch.results.len() >= MAX_RESULTS {
            return Ok(batch);
        }
        let Some(speech) = gate.analyze_recording(&segment.samples, || cancelled.is_cancelled())
        else {
            continue;
        };
        let mut transcript = None;
        if speech.has_speech {
            loop {
                let permit = match input.scheduler.acquire_capture(input.operation, &cancelled) {
                    Ok(permit) => permit,
                    Err(_) if cancelled.is_cancelled() => break,
                    // Stop may close the scheduler lane before the consumer
                    // observes its stop token. Preserve the committed prefix.
                    Err(AppError::Cancelled(_)) if !input.cancellation.is_cancelled() => {
                        return Ok(batch);
                    }
                    Err(error) => return Err(error),
                };
                let result = recognize(
                    &segment.samples,
                    segment.range.start as u64,
                    crate::application::transcription_context::bounded(&context),
                    &permit.cancellation,
                );
                let preempted = permit.cancellation.is_cancelled();
                drop(permit);
                if cancelled.is_cancelled() {
                    break;
                }
                if preempted {
                    continue;
                } // retry only this uncommitted range
                let mut result = result?;
                // Stop needs text/context, not retained timestamp arrays.
                result.segments = Vec::new();
                result.words = Vec::new();
                text_bytes +=
                    result.text.len() + result.detected_language.as_ref().map_or(0, String::len);
                if text_bytes > MAX_TEXT_BYTES {
                    return Ok(batch);
                }
                transcript = Some(result);
                break;
            }
        }
        if cancelled.is_cancelled() {
            continue;
        }
        if let Some(result) = &transcript {
            if let Some(value) = crate::application::transcription_context::bounded(&result.text) {
                context = value.to_owned();
            }
        }
        batch.processed_until = segment.range.end;
        batch.results.push(CaptureSegmentResult {
            operation_id: input.operation,
            index: batch.results.len(),
            range: segment.range,
            transcript,
        });
        completed.store(batch.results.len(), Ordering::Release);
    }
}

impl super::Pipeline {
    pub fn start_capture_segments_for(
        &self,
        operation: u64,
        vad_path: PathBuf,
        start_after_samples: usize,
        recognize: impl FnMut(&[i16], u64, Option<&str>, &OperationCancellation) -> AppResult<WindowTranscript>
            + Send
            + 'static,
    ) -> AppResult<bool> {
        let activity = activity::lease()?;
        let recording = self.recording.lock();
        if !*recording
            || self
                .capture_operation
                .load(std::sync::atomic::Ordering::Acquire)
                != operation
            || !self.is_operation_active(operation)
        {
            return Ok(false);
        }
        let writer = self
            .writer
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Audio("capture writer unavailable".into()))?;
        let cancellation = self
            .cancellation(operation)
            .ok_or_else(|| AppError::Cancelled("capture retired".into()))?;
        self.capture_segments.start(
            CaptureSegmentInput {
                operation,
                profile: CaptureSegmentProfile::from(
                    &self.session_settings_for(operation).unwrap_or_default(),
                ),
                cancellation,
                writer,
                scheduler: self.scheduler(),
                vad_path,
                start_after_samples,
            },
            recognize,
            activity,
        )?;
        Ok(true)
    }

    pub fn take_capture_segments_for(&self, operation: u64) -> Option<CaptureSegmentTask> {
        let recording = self.recording.lock();
        if *recording || !self.is_operation_active(operation) {
            return None;
        }
        let task = self.capture_segments.take(operation)?;
        debug_assert_eq!(task.operation, operation);
        task.stop();
        Some(task)
    }

    pub fn completed_capture_segments_for(&self, operation: u64) -> Option<usize> {
        let _capture = self.recording.lock();
        if !self.is_operation_active(operation) {
            return None;
        }
        self.capture_segments
            .0
            .lock()
            .get(&operation)
            .map(CaptureSegmentTask::completed_segments)
    }
}

#[cfg(test)]
mod tests;
