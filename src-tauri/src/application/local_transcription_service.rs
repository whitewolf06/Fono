//! Tauri composition for the authenticated loopback transcription service.

use std::sync::Arc;

use tauri::AppHandle;

use crate::application::desktop_transcription_runtime::DesktopTranscriptionRuntime;
use crate::application::rest_api::{self, RestApiServer, RestApiState};
use crate::application::transcription_contract::TRANSCRIPTION_PROTOCOL_VERSION;
use crate::application::transcription_jobs::{
    TranscriptionJob, TranscriptionJobQueue, TranscriptionJobWorker, TranscriptionJobs,
    TranscriptionQueueSnapshot,
};
use crate::application::transcription_service::TranscriptionService;
use crate::error::{AppError, AppResult};
use serde::Serialize;

const DEFAULT_API_PORT: u16 = 17_832;
const JOB_QUEUE_CAPACITY: usize = 4;

pub struct LocalTranscriptionService {
    server: RestApiServer,
    worker: TranscriptionJobWorker,
    jobs: Arc<dyn TranscriptionJobs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalTranscriptionServiceSnapshot {
    pub address: String,
    pub protocol_version: u16,
    pub queue: TranscriptionQueueSnapshot,
}

impl LocalTranscriptionService {
    pub fn start(app: AppHandle) -> AppResult<Self> {
        let runtime = DesktopTranscriptionRuntime::new(app);
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(TranscriptionJobQueue::new(
            TranscriptionService::new(runtime.clone()),
            runtime,
            JOB_QUEUE_CAPACITY,
        ));
        let worker = TranscriptionJobWorker::start(Arc::clone(&jobs));
        let token = crate::state::transcription_api_token()?;
        let upload_dir = crate::state::app_data_dir()?.join("transcription-uploads");
        let port = configured_port(std::env::var("FONO_API_PORT").ok())?;
        let server = match tauri::async_runtime::block_on(rest_api::start(
            RestApiState::new(token, TRANSCRIPTION_PROTOCOL_VERSION)
                .with_jobs(Arc::clone(&jobs))
                .with_upload_dir(upload_dir),
            port,
        )) {
            Ok(server) => server,
            Err(error) => {
                worker.shutdown();
                return Err(error.into());
            }
        };
        tracing::info!(address = %server.local_addr(), "local transcription REST service started");
        Ok(Self {
            server,
            worker,
            jobs,
        })
    }

    pub fn snapshot(&self) -> LocalTranscriptionServiceSnapshot {
        LocalTranscriptionServiceSnapshot {
            address: self.server.local_addr().to_string(),
            protocol_version: TRANSCRIPTION_PROTOCOL_VERSION,
            queue: self.jobs.snapshot(),
        }
    }

    pub fn cancel_job(&self, id: &str) -> Option<TranscriptionJob> {
        self.jobs.cancel_job(id)
    }

    pub fn shutdown(&self) {
        self.server.shutdown();
        self.worker.shutdown();
    }
}

impl Drop for LocalTranscriptionService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn configured_port(value: Option<String>) -> AppResult<u16> {
    let Some(value) = value else {
        return Ok(DEFAULT_API_PORT);
    };
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| {
            AppError::Config("FONO_API_PORT must be an integer from 1 through 65535".into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_a_stable_default_port_and_validates_override() {
        assert_eq!(configured_port(None).unwrap(), DEFAULT_API_PORT);
        assert_eq!(configured_port(Some("18000".into())).unwrap(), 18_000);
        assert!(configured_port(Some("0".into())).is_err());
        assert!(configured_port(Some("not-a-port".into())).is_err());
    }
}
