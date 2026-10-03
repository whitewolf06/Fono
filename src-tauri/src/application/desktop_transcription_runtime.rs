//! API jobs share inference capacity while capture remains independent.

#[path = "service_windows.rs"]
mod service_windows;

use tauri::{AppHandle, Manager};

use crate::application::transcription_jobs::InteractiveActivity;
use crate::application::transcription_service::{
    configured_model_identifier, TranscriptionRuntime,
};
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

impl InteractiveActivity for DesktopTranscriptionRuntime {
    fn is_active(&self) -> bool {
        self.app.state::<Pipeline>().current_operation().is_some()
    }
}

impl TranscriptionRuntime for DesktopTranscriptionRuntime {
    fn configured_model(&self) -> AppResult<String> {
        let settings = self.app.state::<AppState>().settings();
        let model_path = settings
            .whisper_model_path
            .ok_or(AppError::ModelNotLoaded)?;
        configured_model_identifier(std::path::Path::new(&model_path))
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        job_cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
        let pipeline = self.app.state::<Pipeline>();
        let (operation_id, operation_cancellation) = pipeline.start_service_transcription()?;
        // Worker request IDs already isolate cancellation. A separate namespace
        // also makes diagnostic operation IDs unambiguous across coordinators.
        let worker_operation_id = operation_id | (1_u64 << 63);
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
            let scheduler = pipeline.scheduler();
            let stt = pipeline.stt();
            let worker_paths = crate::stt::worker_paths_for_app(&self.app);
            service_windows::transcribe_windows(
                pcm_samples,
                &cancellation,
                |window, start, context| {
                    let permit = match scheduler.acquire(false, &cancellation) {
                        Ok(permit) => permit,
                        Err(error) => return (Err(error), false),
                    };
                    let result = (|| {
                        stt.ensure_loaded(
                            std::path::Path::new(&model_path),
                            settings.acceleration,
                            &worker_paths,
                        )?;
                        stt.transcribe_window(
                            window,
                            language,
                            context,
                            worker_operation_id,
                            &permit.cancellation,
                            start,
                        )
                    })();
                    let preempted =
                        permit.cancellation.is_cancelled() && !cancellation.is_cancelled();
                    drop(permit);
                    (result, preempted)
                },
            )
        })();
        let reason = match &result {
            Ok(_) => TerminalReason::Completed,
            Err(AppError::Cancelled(_)) => TerminalReason::Cancelled,
            Err(_) => TerminalReason::Failed,
        };
        pipeline.finish_service_operation(operation_id, reason);
        result
    }
}
