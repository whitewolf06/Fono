//! Typed desktop bridge for the local transcription service monitor.

use tauri::{AppHandle, Manager};

use crate::application::local_transcription_service::{
    LocalTranscriptionService, LocalTranscriptionServiceSnapshot,
};
use crate::application::transcription_jobs::TranscriptionJob;

#[tauri::command]
pub fn get_local_transcription_service_snapshot(
    app: AppHandle,
) -> LocalTranscriptionServiceSnapshot {
    app.state::<LocalTranscriptionService>().snapshot()
}

#[tauri::command]
pub fn cancel_local_transcription_job(app: AppHandle, id: String) -> Option<TranscriptionJob> {
    app.state::<LocalTranscriptionService>().cancel_job(&id)
}
