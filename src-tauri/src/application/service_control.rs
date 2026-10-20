//! Serialized lifecycle for the optional loopback service.
use super::local_transcription_service::{
    LocalTranscriptionService, LocalTranscriptionServiceSnapshot,
};
use crate::{
    error::{AppError, AppResult},
    state::AppState,
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
            match LocalTranscriptionService::start(app.clone(), true) {
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
            enabled: service.as_ref().is_some_and(|s| s.api_enabled()),
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
        let _activity = crate::application::updates::activity::lease()?;
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
        let state = app.state::<AppState>();
        let base = state.settings();
        let mut candidate = base.clone();
        candidate.service_enabled = enabled;
        let previous = service.as_ref().is_some_and(|s| s.api_enabled());
        if enabled && service.is_none() {
            *service = Some(LocalTranscriptionService::start(app.clone(), false)?);
        }
        if let Some(current) = service.as_mut() {
            current.set_api_enabled(enabled)?;
        }
        let settings = match state.persist_settings_delta(&base, &candidate) {
            Ok(settings) => settings,
            Err(error) => {
                if let Some(current) = service.as_mut() {
                    let _ = current.set_api_enabled(previous);
                }
                return Err(error);
            }
        };
        *self.error.lock() = None;
        crate::events::emit_settings(app, &settings);
        crate::events::emit_service_changed(app);
        Ok(())
    }
    pub fn cancel_job(&self, id: &str) -> Option<super::transcription_jobs::TranscriptionJob> {
        let jobs = self.service.lock().as_ref().map(|s| s.jobs())?;
        jobs.cancel_job(id)
    }
    pub fn submit_files(
        &self,
        app: &AppHandle,
        paths: Vec<std::path::PathBuf>,
    ) -> AppResult<super::file_transcription::SubmitFilesResult> {
        if paths.len() > 16 {
            return Err(AppError::Config(
                "Добавляйте не больше 16 файлов за один раз.".into(),
            ));
        }
        if paths.is_empty() {
            return Ok(super::file_transcription::SubmitFilesResult::default());
        }
        let mut service = self.service.lock();
        if service.is_none() {
            *service = Some(LocalTranscriptionService::start(app.clone(), false)?);
        }
        let jobs = service.as_ref().expect("service initialized").jobs();
        drop(service);
        let language = app.state::<AppState>().settings().language;
        let mut result = super::file_transcription::SubmitFilesResult::default();
        for path in paths {
            match jobs.submit_file(path, language.clone()) {
                Ok(job) => result.accepted.push(job.id),
                Err(_) => result.rejected += 1,
            }
        }
        Ok(result)
    }

    pub fn get_job(&self, id: &str) -> Option<super::transcription_jobs::TranscriptionJob> {
        let jobs = self.service.lock().as_ref().map(|s| s.jobs())?;
        jobs.get_job(id)
    }

    pub fn shutdown(&self) {
        let service = self.service.lock().take();
        drop(service);
    }
}
