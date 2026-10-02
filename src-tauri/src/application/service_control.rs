//! Serialized lifecycle for the optional loopback service.
use super::local_transcription_service::{
    LocalTranscriptionService, LocalTranscriptionServiceSnapshot,
};
use crate::{
    error::{AppError, AppResult},
    state::{self, AppState},
};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct ServiceControl {
    service: Mutex<Option<LocalTranscriptionService>>,
    error: Mutex<Option<String>>,
}
#[derive(serde::Serialize)]
pub struct ServiceStatus {
    #[serde(flatten)]
    pub snapshot: LocalTranscriptionServiceSnapshot,
    pub enabled: bool,
    pub error: Option<String>,
}
impl ServiceControl {
    pub fn initialize(&self, app: AppHandle) {
        let mut service = self.service.lock();
        if app.state::<AppState>().settings().service_enabled && service.is_none() {
            match LocalTranscriptionService::start(app.clone()) {
                Ok(started) => *service = Some(started),
                Err(error) => *self.error.lock() = Some(error.to_string()),
            }
        }
        drop(service);
        crate::events::emit_service_changed(&app);
    }
    pub fn snapshot(&self) -> ServiceStatus {
        let service = self.service.lock();
        ServiceStatus {
            enabled: service.is_some(),
            error: self.error.lock().clone(),
            snapshot: service.as_ref().map(|s| s.snapshot()).unwrap_or_else(|| {
                LocalTranscriptionServiceSnapshot {
                    address: format!(
                        "127.0.0.1:{}",
                        std::env::var("FONO_API_PORT").unwrap_or_else(|_| "17832".into())
                    ),
                    protocol_version: super::transcription_contract::TRANSCRIPTION_PROTOCOL_VERSION,
                    queue: super::transcription_jobs::TranscriptionQueueSnapshot {
                        capacity: 4,
                        ..Default::default()
                    },
                    history: crate::service_history::snapshot().unwrap_or_default(),
                }
            }),
        }
    }
    pub fn set_enabled(&self, app: &AppHandle, enabled: bool) -> AppResult<()> {
        let mut service = self.service.lock();
        if !enabled {
            if let Some(current) = service.as_ref() {
                let queue = current.snapshot().queue;
                if queue.queued + queue.preparing + queue.transcribing > 0 {
                    return Err(AppError::Config(
                        "Сначала завершите или отмените задачи API".into(),
                    ));
                }
            }
        }
        let mut settings = app.state::<AppState>().settings();
        if enabled && service.is_none() {
            let next = LocalTranscriptionService::start(app.clone())?;
            settings.service_enabled = true;
            state::save_settings(&settings)?;
            *service = Some(next);
        } else {
            settings.service_enabled = enabled;
            state::save_settings(&settings)?;
            if !enabled {
                service.take();
            }
        }
        *self.error.lock() = None;
        app.state::<AppState>().set_settings(settings.clone());
        crate::events::emit_settings(app, &settings);
        crate::events::emit_service_changed(app);
        Ok(())
    }
    pub fn cancel_job(&self, id: &str) -> Option<super::transcription_jobs::TranscriptionJob> {
        self.service.lock().as_ref().and_then(|s| s.cancel_job(id))
    }
    pub fn shutdown(&self) {
        self.service.lock().take();
    }
}
