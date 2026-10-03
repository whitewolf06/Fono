use std::path::Path;
use std::sync::Arc;

use fono_stt_protocol::{inference::InferenceState, BackendKind, WindowTranscript};
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::error::{AppError, AppResult};
use crate::operation::OperationCancellation;
use crate::types::Transcript;

pub(super) struct EmbeddedEngine {
    inference: InferenceState,
    pub(super) model_path: String,
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
        Ok(Self {
            inference,
            model_path,
            use_gpu,
        })
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
    ) -> AppResult<Transcript> {
        let backend = self.backend();
        let result = self
            .inference
            .transcribe(samples, language, None, 0, backend, false, &|| {
                cancellation.is_cancelled()
            });
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
        language: &str,
        context: Option<&str>,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
    ) -> AppResult<WindowTranscript> {
        let backend = self.backend();
        self.inference
            .transcribe(
                samples,
                language,
                context,
                audio_start_sample,
                backend,
                true,
                &|| cancellation.is_cancelled(),
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
