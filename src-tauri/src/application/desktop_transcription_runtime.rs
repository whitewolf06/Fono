//! Desktop adapter that gives local REST jobs the same STT lease as dictation.

use tauri::{AppHandle, Manager};

use crate::application::transcription_service::TranscriptionRuntime;
use crate::error::{AppError, AppResult};
use crate::operation::TerminalReason;
use crate::pipeline::Pipeline;
use crate::state::AppState;
use crate::types::Transcript;
use fono_core::OperationCancellation;

#[derive(Clone)]
pub struct DesktopTranscriptionRuntime {
    app: AppHandle,
}

impl DesktopTranscriptionRuntime {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl TranscriptionRuntime for DesktopTranscriptionRuntime {
    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        job_cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        let pipeline = self.app.state::<Pipeline>();
        let (operation_id, operation_cancellation) = pipeline.start_service_transcription()?;
        let cancellation = job_cancellation.combined_with(&operation_cancellation);
        let result = (|| {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled(
                    "service job cancelled before model preparation".into(),
                ));
            }
            let settings = self.app.state::<AppState>().settings();
            let model_path = settings
                .whisper_model_path
                .ok_or(AppError::ModelNotLoaded)?;
            let stt = pipeline.stt();
            stt.ensure_loaded(
                std::path::Path::new(&model_path),
                settings.acceleration,
                &crate::stt::worker_paths_for_app(&self.app),
            )?;
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled(
                    "service job cancelled before inference".into(),
                ));
            }
            stt.transcribe_cancellable(pcm_samples, language, cancellation)
        })();
        let reason = match &result {
            Ok(_) => TerminalReason::Completed,
            Err(AppError::Cancelled(_)) => TerminalReason::Cancelled,
            Err(_) => TerminalReason::Failed,
        };
        let _ = pipeline.finish_operation(operation_id, reason);
        result
    }
}
