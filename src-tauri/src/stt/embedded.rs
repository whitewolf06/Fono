use std::path::Path;
use std::sync::Arc;

use fono_stt_protocol::{inference::InferenceState, BackendKind, WindowTranscript};
use parking_lot::Mutex;
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::error::{AppError, AppResult};
use crate::types::{ModelId, Transcript};
use fono_core::OperationCancellation;

use super::recognizer::{
    RecognitionOptions, Recognizer, RecognizerCapabilities, RecognizerLoad, WindowOptions,
};

pub(super) struct EmbeddedRecognizer {
    engine: Mutex<EmbeddedEngine>,
    model: ModelId,
    device: &'static str,
}

impl EmbeddedRecognizer {
    pub(super) fn is_idle(&self) -> bool {
        self.engine.try_lock().is_some()
    }
}

impl Recognizer for EmbeddedRecognizer {
    fn load(request: &RecognizerLoad<'_>) -> AppResult<Self> {
        let super::EngineCandidate::Embedded { use_gpu } = request.candidate else {
            return Err(AppError::Config("Expected an embedded recognizer".into()));
        };
        let engine = EmbeddedEngine::load(Path::new(&request.model.local_path), *use_gpu)?;
        let device = engine.device();
        Ok(Self {
            engine: Mutex::new(engine),
            model: request.model.id.clone(),
            device,
        })
    }

    fn model(&self) -> &ModelId {
        &self.model
    }

    fn device(&self) -> &'static str {
        self.device
    }

    fn capabilities(&self) -> RecognizerCapabilities {
        RecognizerCapabilities::whisper(true)
    }

    fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.engine.lock().transcribe(
            samples,
            options.language,
            cancellation,
            options.decoding,
            options.glossary,
        )
    }

    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        self.engine.lock().transcribe_window(
            samples,
            options.context,
            cancellation,
            options.audio_start_sample,
            options.recognition,
        )
    }
}

pub(super) struct EmbeddedEngine {
    inference: InferenceState,
    pub(super) use_gpu: bool,
}

impl EmbeddedEngine {
    pub(super) fn load(model_path: &Path, use_gpu: bool) -> AppResult<Self> {
        let model_path = model_path.to_string_lossy().to_string();
        let use_gpu = use_gpu && super::gpu_backend_compiled();
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let context = WhisperContext::new_with_params(&model_path, params)
            .map_err(|error| AppError::Stt(format!("WhisperContext: {error}")))?;
        let inference = InferenceState::new(Arc::new(context)).map_err(AppError::Stt)?;
        Ok(Self { inference, use_gpu })
    }

    pub(super) fn device(&self) -> &'static str {
        match self.backend() {
            BackendKind::Cpu => "CPU",
            BackendKind::Cuda => "CUDA",
            BackendKind::Vulkan => "Vulkan",
        }
    }

    fn backend(&self) -> BackendKind {
        if !self.use_gpu {
            return BackendKind::Cpu;
        }
        if cfg!(feature = "cuda") {
            BackendKind::Cuda
        } else if cfg!(feature = "vulkan") {
            BackendKind::Vulkan
        } else {
            BackendKind::Cpu
        }
    }

    pub(super) fn transcribe(
        &mut self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
        decoding: fono_stt_protocol::DecodingOptions,
        glossary: &[String],
    ) -> AppResult<Transcript> {
        let backend = self.backend();
        let signals: Vec<_> = cancellation.atomic_signals().collect();
        let result = self.inference.transcribe_with_glossary(
            samples, language, None, 0, backend, false, &signals, decoding, glossary,
        );
        result.map(super::window::to_transcript).map_err(|message| {
            if cancellation.is_cancelled() {
                AppError::Cancelled(message)
            } else {
                AppError::Stt(message)
            }
        })
    }

    pub(super) fn transcribe_window(
        &mut self,
        samples: &[i16],
        context: Option<&str>,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
        options: RecognitionOptions<'_>,
    ) -> AppResult<WindowTranscript> {
        let backend = self.backend();
        let signals: Vec<_> = cancellation.atomic_signals().collect();
        self.inference
            .transcribe_with_glossary(
                samples,
                options.language,
                context,
                audio_start_sample,
                backend,
                true,
                &signals,
                options.decoding,
                options.glossary,
            )
            .map_err(|message| {
                if cancellation.is_cancelled() {
                    AppError::Cancelled(message)
                } else {
                    AppError::Stt(message)
                }
            })
    }
}
