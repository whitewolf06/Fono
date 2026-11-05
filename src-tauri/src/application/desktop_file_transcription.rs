//! Trusted desktop file route. HTTP uploads enter the PCM/API route instead.

use std::sync::Arc;

use fono_core::{OperationCancellation, TerminalReason};
use fono_stt::remote::RemoteTranscriptionClient;
use fono_types::{
    TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
    TranscriptionServiceResult,
};
use tauri::Manager;

use super::DesktopTranscriptionRuntime;
use crate::application::transcription_service::TranscriptionService;
use crate::types::{RemoteSttSettings, Settings, SttSource};

pub(super) fn transcribe(
    runtime: &DesktopTranscriptionRuntime,
    request: TranscriptionRequest,
    cancellation: OperationCancellation,
    progress: &(dyn Fn(f32) + Sync),
) -> TranscriptionServiceResult<TranscriptionResult> {
    let settings = runtime.app.state::<crate::state::AppState>().settings();
    route(
        settings,
        request,
        cancellation,
        |request, cancellation| {
            TranscriptionService::new(runtime.clone()).transcribe_with_progress(
                request,
                cancellation,
                progress,
            )
        },
        |remote, request, cancellation| {
            transcribe_remote(runtime, remote, request, cancellation, progress)
        },
    )
}

/// Settings and credentials cannot change while the queue or the service
/// operation is active. Keep this owned selection for the entire remote job.
fn route(
    settings: Settings,
    request: TranscriptionRequest,
    cancellation: OperationCancellation,
    local: impl FnOnce(
        TranscriptionRequest,
        OperationCancellation,
    ) -> TranscriptionServiceResult<TranscriptionResult>,
    remote: impl FnOnce(
        RemoteSttSettings,
        TranscriptionRequest,
        OperationCancellation,
    ) -> TranscriptionServiceResult<TranscriptionResult>,
) -> TranscriptionServiceResult<TranscriptionResult> {
    if cancellation.is_cancelled() {
        return Err(TranscriptionServiceError::Cancelled(
            "file transcription cancelled before source selection".into(),
        ));
    }
    match settings.stt_source {
        SttSource::Local => local(request, cancellation),
        SttSource::FonoServer => {
            let selected = settings.remote_stt.ok_or_else(|| {
                TranscriptionServiceError::ModelNotReady(
                    "Fono Server connection is not configured".into(),
                )
            })?;
            if request
                .model
                .as_deref()
                .is_some_and(|requested| requested != selected.model.public_id())
            {
                return Err(TranscriptionServiceError::InvalidRequest(
                    "model must match the selected Fono Server model".into(),
                ));
            }
            remote(selected, request, cancellation)
        }
    }
}

fn transcribe_remote(
    runtime: &DesktopTranscriptionRuntime,
    remote: RemoteSttSettings,
    request: TranscriptionRequest,
    cancellation: OperationCancellation,
    progress: &(dyn Fn(f32) + Sync),
) -> TranscriptionServiceResult<TranscriptionResult> {
    let pipeline = runtime.app.state::<crate::pipeline::Pipeline>();
    let (operation_id, operation_cancellation) = pipeline
        .start_service_transcription()
        .map_err(TranscriptionServiceError::from)?;
    let cancellation = cancellation.combined_with(&operation_cancellation);
    // No local STT permit or model load: the server's Batch scheduler yields
    // its own inference windows to interactive /v1/recognize requests.
    let result = (|| {
        if cancellation.is_cancelled() {
            return Err(TranscriptionServiceError::Cancelled(
                "file transcription cancelled before connection".into(),
            ));
        }
        let endpoint =
            crate::stt::remote_endpoint(&remote).map_err(TranscriptionServiceError::from)?;
        let client = RemoteTranscriptionClient::connect(
            endpoint,
            remote.model,
            Arc::new(crate::stt::DesktopActivity),
            &cancellation,
        )
        .map_err(remote_error)?;
        client
            .transcribe_pcm_job(
                &request.pcm_samples,
                &request.language,
                &cancellation,
                progress,
            )
            .map_err(remote_error)
    })();
    let reason = match &result {
        Ok(_) => TerminalReason::Completed,
        Err(TranscriptionServiceError::Cancelled(_)) => TerminalReason::Cancelled,
        Err(_) => TerminalReason::Failed,
    };
    pipeline.finish_service_operation(operation_id, reason);
    result
}

fn remote_error(error: fono_stt::error::SttError) -> TranscriptionServiceError {
    use fono_types::{CoreError, ErrorCode};
    let error = CoreError::from(&error);
    match error.code {
        ErrorCode::Cancelled => TranscriptionServiceError::Cancelled(error.message),
        ErrorCode::Busy => TranscriptionServiceError::Busy(error.message),
        ErrorCode::ModelNotFound => TranscriptionServiceError::ModelNotFound(error.message),
        ErrorCode::ModelNotReady => TranscriptionServiceError::ModelNotReady(error.message),
        ErrorCode::InvalidRequest | ErrorCode::UnsupportedAudio => {
            TranscriptionServiceError::InvalidRequest(error.message)
        }
        ErrorCode::WorkerFailed => TranscriptionServiceError::WorkerFailed(error.message),
        _ => TranscriptionServiceError::Failed(error.message),
    }
}

#[cfg(test)]
#[path = "desktop_file_transcription_tests.rs"]
mod tests;
