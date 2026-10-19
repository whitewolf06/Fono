//! API jobs share inference capacity while capture remains independent.

#[path = "service_windows.rs"]
mod service_windows;

use tauri::{AppHandle, Manager};

use crate::application::transcription_jobs::InteractiveActivity;
use crate::application::transcription_service::TranscriptionRuntime;
use crate::error::{AppError, AppResult};
use crate::pipeline::Pipeline;
use crate::state::AppState;
use crate::types::Transcript;
use fono_core::OperationCancellation;
use fono_core::TerminalReason;

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
        settings
            .selected_stt_model()
            .map(|m| m.id.public_id())
            .ok_or(AppError::ModelNotLoaded)
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
            let decoding = settings.decoding_options();
            let model = settings
                .stt_model
                .clone()
                .or_else(|| {
                    settings
                        .whisper_model_path
                        .clone()
                        .map(crate::types::SttModelSelection::from_whisper_path)
                })
                .ok_or(AppError::ModelNotLoaded)?;
            let scheduler = pipeline.scheduler();
            let stt = pipeline.stt();
            let worker_paths = crate::stt::worker_paths_for_app(&self.app);
            #[cfg(feature = "offline-stt")]
            if model.id.engine != crate::types::SttEngine::Whisper {
                let vad = worker_paths
                    .speech_vad
                    .as_deref()
                    .ok_or_else(|| AppError::Stt("Speech VAD model is unavailable".into()))?;
                let segments = fono_audio::speech_segments::segment_recording(
                    pcm_samples,
                    vad,
                    25 * 16_000,
                    || cancellation.is_cancelled(),
                )
                .map_err(|e| AppError::Stt(e.to_string()))?
                .ok_or_else(|| AppError::Cancelled("service segmentation cancelled".into()))?;
                let result = super::segmented_transcription::transcribe_segments(
                    pcm_samples,
                    &segments,
                    &cancellation,
                    |window, start| {
                        let permit = match scheduler.acquire(false, &cancellation) {
                            Ok(permit) => permit,
                            Err(error) => return (Err(error), false),
                        };
                        let result = stt
                            .ensure_model_loaded(
                                &model,
                                settings.acceleration,
                                &worker_paths,
                                &permit.cancellation,
                            )
                            .and_then(|()| {
                                stt.transcribe_window_with_options(
                                    window,
                                    language,
                                    None,
                                    worker_operation_id,
                                    &permit.cancellation,
                                    start,
                                    decoding,
                                )
                            });
                        let preempted =
                            permit.cancellation.is_cancelled() && !cancellation.is_cancelled();
                        drop(permit);
                        (result, preempted)
                    },
                )?;
                return Ok(Transcript {
                    text: result.text,
                    detected_language: result.detected_language,
                    transcribe_secs: Some(result.transcribe_secs),
                    audio_secs: Some(result.audio_secs),
                    device: Some(
                        match result.backend {
                            fono_stt_protocol::BackendKind::Cpu => "CPU",
                            fono_stt_protocol::BackendKind::Cuda => "CUDA",
                            fono_stt_protocol::BackendKind::Vulkan => "Vulkan",
                        }
                        .into(),
                    ),
                });
            }
            service_windows::transcribe_windows(
                pcm_samples,
                &cancellation,
                |window, start, context| {
                    let permit = match scheduler.acquire(false, &cancellation) {
                        Ok(permit) => permit,
                        Err(error) => return (Err(error), false),
                    };
                    let result = (|| {
                        stt.ensure_model_loaded(
                            &model,
                            settings.acceleration,
                            &worker_paths,
                            &permit.cancellation,
                        )?;
                        stt.transcribe_window_with_options(
                            window,
                            language,
                            context,
                            worker_operation_id,
                            &permit.cancellation,
                            start,
                            decoding,
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
