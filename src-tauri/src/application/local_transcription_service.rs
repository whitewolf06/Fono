//! Tauri composition for the authenticated loopback transcription service.

use std::sync::Arc;

use tauri::AppHandle;

use crate::application::desktop_transcription_runtime::DesktopTranscriptionRuntime;
use crate::application::rest_api::{self, RestApiServer, RestApiState};
use crate::application::transcription_contract::TRANSCRIPTION_PROTOCOL_VERSION;
use crate::application::transcription_jobs::{
    TranscriptionJobQueue, TranscriptionJobWorker, TranscriptionJobs, TranscriptionQueueSnapshot,
};
use crate::application::transcription_service::TranscriptionService;
use crate::error::{AppError, AppResult};
use serde::Serialize;

const DEFAULT_API_PORT: u16 = 17_832;
const JOB_QUEUE_CAPACITY: usize = 4;

pub struct LocalTranscriptionService {
    app: AppHandle,
    server: Option<RestApiServer>,
    worker: TranscriptionJobWorker,
    jobs: Arc<dyn TranscriptionJobs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalTranscriptionServiceSnapshot {
    pub address: String,
    pub protocol_version: u16,
    pub queue: TranscriptionQueueSnapshot,
    pub history: crate::service_history::ServiceHistorySnapshot,
}

impl LocalTranscriptionService {
    pub fn start(app: AppHandle, api_enabled: bool) -> AppResult<Self> {
        let runtime = DesktopTranscriptionRuntime::new(app.clone());
        let event_app = app.clone();
        let queue = Arc::new(
            TranscriptionJobQueue::new(
                TranscriptionService::new(runtime.clone()),
                runtime,
                JOB_QUEUE_CAPACITY,
            )
            .with_observer(Arc::new(move |job| {
                if let Err(error) = crate::service_history::upsert_terminal(job) {
                    tracing::error!(%error, "could not persist local transcription history");
                }
                crate::events::emit_service_changed(&event_app);
            })),
        );
        let jobs: Arc<dyn TranscriptionJobs> = queue;
        let worker = TranscriptionJobWorker::start(Arc::clone(&jobs))?;
        let mut service = Self {
            app,
            server: None,
            worker,
            jobs,
        };
        if api_enabled {
            service.set_api_enabled(true)?;
        }
        Ok(service)
    }

    pub fn api_enabled(&self) -> bool {
        self.server.is_some()
    }

    pub fn set_api_enabled(&mut self, enabled: bool) -> AppResult<()> {
        if !enabled {
            if let Some(server) = self.server.take() {
                server.shutdown();
            }
        } else if self.server.is_none() {
            let token = crate::state::transcription_api_token()?;
            let upload_dir = crate::state::app_data_dir()?.join("transcription-uploads");
            let port = configured_port(std::env::var("FONO_API_PORT").ok())?;
            let server = tauri::async_runtime::block_on(rest_api::start(
                RestApiState::new(token, TRANSCRIPTION_PROTOCOL_VERSION)
                    .with_jobs(Arc::clone(&self.jobs))
                    .with_upload_dir(upload_dir),
                port,
            ))?;
            tracing::info!(address = %server.local_addr(), "local transcription REST service started");
            self.server = Some(server);
        }
        Ok(())
    }

    pub fn jobs(&self) -> Arc<dyn TranscriptionJobs> {
        Arc::clone(&self.jobs)
    }

    pub fn snapshot(&self) -> LocalTranscriptionServiceSnapshot {
        LocalTranscriptionServiceSnapshot {
            address: self
                .server
                .as_ref()
                .map(|s| s.local_addr().to_string())
                .unwrap_or_else(|| {
                    format!(
                        "127.0.0.1:{}",
                        configured_port(std::env::var("FONO_API_PORT").ok())
                            .unwrap_or(DEFAULT_API_PORT)
                    )
                }),
            protocol_version: TRANSCRIPTION_PROTOCOL_VERSION,
            queue: self.jobs.snapshot(),
            history: crate::service_history::snapshot().unwrap_or_else(|error| {
                tracing::error!(%error, "could not load local transcription history");
                crate::service_history::ServiceHistorySnapshot::default()
            }),
        }
    }

    pub fn clear_history(&self) -> AppResult<()> {
        crate::service_history::clear()?;
        crate::events::emit_service_changed(&self.app);
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Some(server) = &self.server {
            server.shutdown();
        }
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
