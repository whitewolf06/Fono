//! Settings-only raw capture: no desktop insertion, archive or last-session publication.
use super::{
    capture::ensure_capture_allowed,
    lifecycle::{OperationScope, Session},
    recognition,
};
use crate::{
    error::{AppError, AppResult},
    operation::OperationSource,
    pipeline::Pipeline,
    state::AppState,
};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager};

const MAX_TEST_SECONDS: u64 = 120;

struct CaptureSlot<T> {
    id: u64,
    owner: Option<T>,
}

impl<T> CaptureSlot<T> {
    fn claim(&mut self, id: u64) -> AppResult<T> {
        if id != self.id {
            return Err(stale());
        }
        self.owner.take().ok_or_else(|| {
            AppError::Busy("Тестовая диктовка уже распознаётся. Дождитесь результата.".into())
        })
    }
}

#[derive(Default)]
pub(crate) struct ProcessingCaptureRuntime(Mutex<Option<CaptureSlot<OperationScope>>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureStarted {
    session_id: u64,
}

#[derive(Serialize)]
pub(crate) struct CaptureResult {
    text: String,
}

fn stale() -> AppError {
    AppError::Cancelled("Эта тестовая запись уже завершена или отменена.".into())
}

pub(crate) fn start(app: AppHandle) -> AppResult<CaptureStarted> {
    let _admission = crate::application::capture_configuration::begin_capture()?;
    ensure_capture_allowed(&app)?;
    let runtime = app.state::<ProcessingCaptureRuntime>();
    let mut current = runtime.0.lock();
    if current.is_some() {
        return Err(AppError::Busy(
            "Сначала завершите текущую тестовую диктовку.".into(),
        ));
    }
    let settings = app.state::<AppState>().settings();
    if settings.whisper_model_path.is_none() {
        return Err(AppError::Stt(
            "Выберите установленную модель распознавания в настройках аудио.".into(),
        ));
    }
    let pipeline = app.state::<Pipeline>();
    let id = pipeline.start_recording_from(
        settings.audio_device_id.as_deref(),
        OperationSource::Diagnostics,
    )?;
    let mut owner = OperationScope::new(app.clone(), id, true);
    if !pipeline.set_session_settings_for(id, settings) {
        owner.complete(&Err::<(), _>(stale()));
        return Err(stale());
    }
    *current = Some(CaptureSlot {
        id,
        owner: Some(owner),
    });
    drop(current);
    let timeout_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(MAX_TEST_SECONDS)).await;
        // An old timer can never cancel a later capture or ordinary dictation.
        let _ = cancel(&timeout_app, id);
    });
    Ok(CaptureStarted { session_id: id })
}

struct Completion {
    app: AppHandle,
    id: u64,
}
impl Drop for Completion {
    fn drop(&mut self) {
        let runtime = self.app.state::<ProcessingCaptureRuntime>();
        let mut current = runtime.0.lock();
        if current.as_ref().is_some_and(|slot| slot.id == self.id) {
            current.take();
        }
    }
}

pub(crate) async fn finish(app: AppHandle, id: u64) -> AppResult<CaptureResult> {
    let owner = app
        .state::<ProcessingCaptureRuntime>()
        .0
        .lock()
        .as_mut()
        .ok_or_else(stale)?
        .claim(id)?;
    // Dropping an IPC caller leaves one cleanup owner; Cancel still fences its session.
    tauri::async_runtime::spawn(async move {
        let _completion = Completion {
            app: app.clone(),
            id,
        };
        let mut owner = owner;
        let result = recognize(&app, id).await;
        owner.complete(&result);
        result
    })
    .await
    .map_err(|error| AppError::Internal(format!("test dictation join: {error}")))?
}

async fn recognize(app: &AppHandle, id: u64) -> AppResult<CaptureResult> {
    let session = Session::current(app, id)?;
    let samples = app
        .state::<Pipeline>()
        .stop_recording_for(id)?
        .ok_or_else(stale)?;
    if samples.is_empty() {
        return Ok(CaptureResult {
            text: String::new(),
        });
    }
    let gate_app = app.clone();
    let cancellation = session.cancellation.clone();
    let activity = crate::application::updates::activity::lease()?;
    let samples = tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let mut gate = crate::application::speech_gate::SpeechGate::new(&gate_app)?;
        Ok::<_, AppError>(
            gate.recording_has_speech(&samples, || cancellation.is_cancelled())
                .then_some(samples),
        )
    })
    .await
    .map_err(|error| AppError::Internal(format!("test speech gate join: {error}")))??;
    if !session.active("processing test after speech validation") {
        return Err(stale());
    }
    let Some(samples) = samples else {
        return Ok(CaptureResult {
            text: String::new(),
        });
    };
    let transcript = recognition::run(&session, samples)
        .await?
        .ok_or_else(stale)?;
    Ok(CaptureResult {
        text: transcript.text,
    })
}

pub(crate) fn cancel(app: &AppHandle, id: u64) -> AppResult<()> {
    let runtime = app.state::<ProcessingCaptureRuntime>();
    let mut current = runtime.0.lock();
    let Some(slot) = current.as_mut().filter(|slot| slot.id == id) else {
        return Ok(());
    };
    let mut owner = slot.owner.take();
    app.state::<Pipeline>().cancel_for(id);
    let stopped = app.state::<Pipeline>().stop_recording_for(id);
    if owner.is_some() {
        current.take();
    }
    drop(current);
    if let Some(owner) = owner.as_mut() {
        owner.complete(&Err::<(), _>(stale()));
    }
    drop(owner);
    stopped.map(|_| ())
}

#[cfg(test)]
#[path = "processing_test_tests.rs"]
mod tests;
