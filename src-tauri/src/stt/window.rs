use crate::error::{AppError, AppResult};
use crate::operation::OperationCancellation;
use crate::types::Transcript;
use fono_stt_protocol::{BackendKind, WindowTranscript};

use super::{ActiveEngine, EngineState, SttEngine};

pub(super) fn to_transcript(result: WindowTranscript) -> Transcript {
    Transcript {
        text: result.text,
        detected_language: result.detected_language,
        transcribe_secs: Some(result.transcribe_secs),
        audio_secs: Some(result.audio_secs),
        device: Some(
            match result.backend {
                BackendKind::Cpu => "CPU",
                BackendKind::Cuda => "CUDA",
                BackendKind::Vulkan => "Vulkan",
            }
            .into(),
        ),
    }
}

impl SttEngine {
    pub fn transcribe_window(
        &self,
        samples: &[i16],
        language: &str,
        context: Option<&str>,
        operation_id: u64,
        cancellation: &OperationCancellation,
        audio_start_sample: u64,
    ) -> AppResult<WindowTranscript> {
        if samples.len() > 30 * 16_000 {
            return Err(AppError::Stt("streaming window exceeds 30 seconds".into()));
        }
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "window cancelled before inference".into(),
            ));
        }
        let active = self.state.lock().active().ok_or(AppError::ModelNotLoaded)?;
        let result = match &active {
            ActiveEngine::Embedded(engine) => engine.lock().transcribe_window(
                samples,
                language,
                context,
                cancellation,
                audio_start_sample,
            ),
            ActiveEngine::Worker(session) => session.transcribe_window(
                samples,
                language,
                context,
                operation_id,
                cancellation,
                audio_start_sample,
            ),
        };
        if result.is_err() && active.is_worker() && !matches!(result, Err(AppError::Cancelled(_))) {
            let mut state = self.state.lock();
            if state.contains(&active) {
                *state = EngineState::Empty;
            }
        }
        result
    }
}
