//! Loaded recognizer contract. Scheduling and operation ownership stay with
//! the caller; a backend call returns only after its native work has finished.
use fono_core::OperationCancellation;
use fono_stt_protocol::{DecodingOptions, WindowTranscript};
use serde::Serialize;

use super::{
    embedded::EmbeddedRecognizer, EngineCandidate, EngineState, WorkerCleanupSupervisor,
    WorkerMailbox,
};
use crate::{
    error::{AppError, AppResult},
    types::{ModelId, SttModelSelection, Transcript},
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationSupport {
    Cooperative,
    BetweenSegments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecognizerCapabilities {
    pub max_segment_samples: usize,
    pub window_transcription: bool,
    pub initial_prompt: bool,
    pub language_detection: bool,
    pub automatic_language: bool,
    pub token_timestamps: bool,
    pub decoding_options: bool,
    pub cancellation: CancellationSupport,
}

impl RecognizerCapabilities {
    pub(super) fn whisper(decoding_options: bool) -> Self {
        Self {
            max_segment_samples: 30 * 16_000,
            window_transcription: true,
            initial_prompt: true,
            language_detection: true,
            automatic_language: true,
            token_timestamps: true,
            decoding_options,
            cancellation: CancellationSupport::Cooperative,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct RecognitionOptions<'a> {
    pub language: &'a str,
    pub decoding: DecodingOptions,
}

#[derive(Clone, Copy)]
pub(super) struct WindowOptions<'a> {
    pub recognition: RecognitionOptions<'a>,
    pub context: Option<&'a str>,
    pub operation_id: u64,
    pub audio_start_sample: u64,
}

pub(super) struct RecognizerLoad<'a> {
    pub model: &'a SttModelSelection,
    pub candidate: &'a EngineCandidate,
    pub cleanup: Arc<WorkerCleanupSupervisor>,
    pub cancellation: &'a OperationCancellation,
    #[cfg(feature = "offline-stt")]
    pub speech_vad: Option<&'a std::path::Path>,
}

/// One synchronous interface for embedded, process-isolated and future offline
/// recognizers. `load` is a factory, so a failed replacement cannot mutate the
/// current runtime. Handles retain their own model/device, never live settings.
pub(super) trait Recognizer: Send + Sync {
    fn load(request: &RecognizerLoad<'_>) -> AppResult<Self>
    where
        Self: Sized;

    fn model(&self) -> &ModelId;
    fn device(&self) -> &'static str;
    fn capabilities(&self) -> RecognizerCapabilities;
    fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript>;
    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript>;
}

pub(super) fn check_cancellation(cancellation: &OperationCancellation) -> AppResult<()> {
    if cancellation.is_cancelled() {
        Err(AppError::Cancelled("STT request cancelled".into()))
    } else {
        Ok(())
    }
}

/// Keep admission until the actual backend returns, even for a backend that
/// cannot interrupt a native decode. A cancelled late result never escapes.
pub(super) fn run<T>(
    cancellation: &OperationCancellation,
    work: impl FnOnce() -> AppResult<T>,
) -> AppResult<T> {
    check_cancellation(cancellation)?;
    let _activity = crate::application::updates::activity::lease()?;
    let result = work();
    check_cancellation(cancellation)?;
    result
}

pub(super) fn load(request: RecognizerLoad<'_>) -> AppResult<EngineState> {
    run(request.cancellation, || {
        super::validate_model_selection(request.model)?;
        match request.candidate {
            EngineCandidate::Embedded { use_gpu } => {
                let engine = EmbeddedRecognizer::load(&request)?;
                let device = engine.device();
                Ok(EngineState::Embedded {
                    engine: Arc::new(engine),
                    device,
                    model_path: request.model.local_path.clone(),
                    use_gpu: *use_gpu,
                })
            }
            EngineCandidate::Worker { .. } => {
                let session = WorkerMailbox::load(&request)?;
                let device = Recognizer::device(&session);
                Ok(EngineState::Worker {
                    session: Arc::new(session),
                    device,
                })
            }
            #[cfg(feature = "offline-stt")]
            EngineCandidate::Offline => {
                let engine = super::offline::SherpaRecognizer::load(&request)?;
                Ok(EngineState::Offline {
                    engine: Arc::new(engine),
                    model_path: request.model.local_path.clone(),
                })
            }
        }
    })
}

fn validate(recognizer: &dyn Recognizer, options: RecognitionOptions<'_>) -> AppResult<()> {
    options.decoding.validate().map_err(AppError::Config)?;
    let capabilities = recognizer.capabilities();
    if !options.decoding.is_default() && !capabilities.decoding_options {
        return Err(AppError::Config(
            "Recognizer does not support the selected decoding options".into(),
        ));
    }
    if options.language == "auto" && !capabilities.automatic_language {
        return Err(AppError::Config(
            "Recognizer does not support automatic language detection".into(),
        ));
    }
    Ok(())
}

pub(super) fn transcribe(
    recognizer: &dyn Recognizer,
    samples: &[i16],
    options: RecognitionOptions<'_>,
    cancellation: &OperationCancellation,
) -> AppResult<Transcript> {
    run(cancellation, || {
        validate(recognizer, options)?;
        recognizer.transcribe(samples, options, cancellation)
    })
}

pub(super) fn transcribe_window(
    recognizer: &dyn Recognizer,
    samples: &[i16],
    options: WindowOptions<'_>,
    cancellation: &OperationCancellation,
) -> AppResult<WindowTranscript> {
    run(cancellation, || {
        validate(recognizer, options.recognition)?;
        let capabilities = recognizer.capabilities();
        if !capabilities.window_transcription {
            return Err(AppError::Config(
                "Recognizer does not support window transcription".into(),
            ));
        }
        if options.context.is_some() && !capabilities.initial_prompt {
            return Err(AppError::Config(
                "Recognizer does not support an initial prompt".into(),
            ));
        }
        recognizer.transcribe_window(samples, options, cancellation)
    })
}

#[cfg(test)]
#[path = "recognizer_tests.rs"]
mod tests;
