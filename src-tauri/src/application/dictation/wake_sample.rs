//! A session-fenced raw capture, separate from dictation, calibration and model inference.
use super::{capture::ensure_capture_allowed, lifecycle::OperationScope};
use crate::{
    application::{
        updates::activity,
        wake_samples::{self, Sample, SampleRequest},
    },
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
};
use fono_core::OperationSource;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager};

struct Slot {
    id: u64,
    request: SampleRequest,
    owner: Option<OperationScope>,
    cancelled: bool,
    ready: bool,
}
pub(crate) struct Runtime {
    slot: Mutex<Option<Slot>>,
    cancellation: Mutex<Option<(u64, fono_core::OperationCancellation)>>,
    session: String,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
            cancellation: Mutex::new(None),
            session: uuid::Uuid::new_v4().to_string(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Started {
    pub session_id: u64,
    pub duration_ms: u64,
}
pub(crate) struct Reservation {
    pub id: u64,
    device: Option<String>,
    duration_ms: u64,
    pub lease: activity::ActivityLease<'static>,
}
fn stale() -> AppError {
    AppError::Cancelled("Запись образца отменена или уже завершена.".into())
}
// Memory-only admission fixes identity before native device work is queued.
pub(crate) fn reserve(app: &AppHandle, request: SampleRequest) -> AppResult<Reservation> {
    request.validate()?;
    let lease = activity::lease()?;
    let _admission = crate::application::capture_configuration::try_begin_capture()?;
    ensure_capture_allowed(app)?;
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.slot.lock();
    if slot.is_some() {
        return Err(AppError::Busy("Сначала завершите запись образца.".into()));
    }
    let settings = app.state::<AppState>().settings();
    let pipeline = app.state::<Pipeline>();
    let id = pipeline.reserve_capture(OperationSource::Diagnostics)?;
    let owner = OperationScope::new(app.clone(), id, true);
    *runtime.cancellation.lock() = pipeline.cancellation(id).map(|token| (id, token));
    let duration_ms = request.duration_ms();
    *slot = Some(Slot {
        id,
        request,
        owner: Some(owner),
        cancelled: false,
        ready: false,
    });
    Ok(Reservation {
        id,
        device: settings.audio_device_id,
        duration_ms,
        lease,
    })
}
pub(crate) fn start(
    app: &AppHandle,
    id: u64,
    device: Option<&str>,
    duration_ms: u64,
) -> AppResult<Started> {
    let result = (|| {
        let _admission = crate::application::capture_configuration::begin_capture()?;
        ensure_capture_allowed(app)?;
        let runtime = app.state::<Runtime>();
        let mut slot = runtime.slot.lock();
        let current = slot
            .as_mut()
            .filter(|slot| slot.id == id && !slot.cancelled)
            .ok_or_else(stale)?;
        app.state::<Pipeline>()
            .start_reserved_capture_with(id, device, &[], None)?;
        current.ready = true;
        Ok(Started {
            session_id: id,
            duration_ms,
        })
    })();
    if result.is_err() {
        let _ = cancel(app, id);
    }
    result
}
pub(crate) async fn start_reserved(app: AppHandle, reservation: Reservation) -> AppResult<Started> {
    let Reservation {
        id,
        device,
        duration_ms,
        lease,
    } = reservation;
    // Bounded even when a webview disappears before it receives the start response.
    let timeout_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(duration_ms + 5_000)).await;
        let cleanup = activity::cleanup_lease();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _cleanup = cleanup;
            cancel(&timeout_app, id)
        })
        .await;
    });
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        start(&app, id, device.as_deref(), duration_ms)
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось запустить запись образца.".into()))?
}
struct Completion {
    app: AppHandle,
    id: u64,
}
impl Drop for Completion {
    fn drop(&mut self) {
        let runtime = self.app.state::<Runtime>();
        let mut slot = runtime.slot.lock();
        if slot.as_ref().is_some_and(|s| s.id == self.id) {
            slot.take();
            runtime.cancellation.lock().take();
        }
    }
}
pub(crate) fn finish(app: AppHandle, id: u64) -> AppResult<Sample> {
    let runtime = app.state::<Runtime>();
    let (mut owner, request) = {
        let mut slot = runtime.slot.lock();
        let current = slot
            .as_mut()
            .filter(|s| s.id == id && !s.cancelled && s.ready)
            .ok_or_else(stale)?;
        let owner = current
            .owner
            .take()
            .ok_or_else(|| AppError::Busy("Образец уже сохраняется.".into()))?;
        (owner, current.request.clone())
    };
    // Completion must run after OperationScope has stopped/settled this operation.
    let completion = Completion {
        app: app.clone(),
        id,
    };
    let result = (|| {
        let pipeline = app.state::<Pipeline>();
        if !pipeline.is_operation_active(id) {
            return Err(stale());
        }
        let samples = pipeline.stop_recording_for(id)?.ok_or_else(stale)?;
        if pipeline
            .capture_dropped_packets_for(id)
            .is_some_and(|count| count > 0)
        {
            return Err(AppError::Audio(
                "Во время записи потерян звук. Повторите образец.".into(),
            ));
        }
        let slot = runtime.slot.lock();
        if !slot.as_ref().is_some_and(|s| s.id == id && !s.cancelled)
            || !pipeline.is_operation_active(id)
        {
            return Err(stale());
        }
        let cancellation = pipeline.cancellation(id).ok_or_else(stale)?;
        // The cancellation token remains writable while filesystem preparation holds the slot.
        wake_samples::save(request, &samples, &runtime.session, || {
            !cancellation.is_cancelled()
        })
    })();
    owner.complete(&result);
    drop(owner);
    drop(completion);
    result
}
pub(crate) fn cancel(app: &AppHandle, id: u64) -> AppResult<()> {
    request_cancel(app, id);
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.slot.lock();
    let Some(current) = slot.as_mut().filter(|s| s.id == id) else {
        return Ok(());
    };
    current.cancelled = true;
    let mut owner = current.owner.take();
    app.state::<Pipeline>().cancel_for(id);
    let stopped = app.state::<Pipeline>().stop_recording_for(id);
    if let Some(owner) = owner.as_mut() {
        owner.complete(&Err::<(), _>(stale()));
    }
    let was_recording = owner.is_some();
    drop(owner);
    if was_recording {
        slot.take();
        runtime.cancellation.lock().take();
    }
    stopped.map(|_| ())
}

/// Memory-only cancellation admission; never waits for device or filesystem I/O.
pub(crate) fn request_cancel(app: &AppHandle, id: u64) {
    if let Some((_, cancellation)) = app
        .state::<Runtime>()
        .cancellation
        .lock()
        .as_ref()
        .filter(|(owner, _)| *owner == id)
    {
        cancellation.cancel();
    }
}
