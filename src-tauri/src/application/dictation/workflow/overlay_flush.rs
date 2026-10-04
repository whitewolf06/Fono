//! A physical shortcut waits for the renderer's latest queued processing choice.
use crate::{
    application::updates::activity,
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
    types::HotkeyMode,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

type PendingFlush = (u64, u64, oneshot::Sender<AppResult<()>>);

#[derive(Default)]
pub(super) struct FlushRuntime {
    next: AtomicU64,
    pending: Mutex<Option<PendingFlush>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    session_id: u64,
    request_id: u64,
}

pub(crate) async fn flush(app: &AppHandle, session_id: u64) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    if settings.hotkey_mode != HotkeyMode::Toggle
        || !settings.overlay_quick_processing
        || !settings.overlay_enabled
    {
        return Ok(());
    }
    let _activity = activity::lease()?;
    let pipeline = app.state::<Pipeline>();
    let cancellation = pipeline
        .cancellation(session_id)
        .ok_or_else(|| AppError::Cancelled("Диктовка уже завершена".into()))?;
    let runtime = app.state::<super::Runtime>();
    let request_id = runtime.flush.next.fetch_add(1, Ordering::Relaxed) + 1;
    let (sender, receiver) = oneshot::channel();
    {
        let mut pending = runtime.flush.pending.lock();
        if pending.is_some() {
            return Err(AppError::Busy(
                "Подтверждение настроек уже выполняется".into(),
            ));
        }
        *pending = Some((session_id, request_id, sender));
    }
    let guard = FlushGuard {
        app: app.clone(),
        request_id,
    };
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::Internal("Индикатор записи недоступен".into()))?;
    window.emit(
        "overlay-processing-flush",
        Request {
            session_id,
            request_id,
        },
    )?;
    let result = tokio::select! {
        response = tokio::time::timeout(std::time::Duration::from_secs(3), receiver) => match response {
            Ok(Ok(result)) => result,
            _ => Err(AppError::Busy("Не удалось подтвердить последние настройки. Запись продолжается; повторите завершение".into())),
        },
        _ = super::super::wait_for_cancellation(cancellation) => Err(AppError::Cancelled("Диктовка отменена".into())),
    };
    drop(guard);
    result
}
pub(crate) fn acknowledge(
    app: &AppHandle,
    session_id: u64,
    request_id: u64,
    error: Option<String>,
) {
    let runtime = app.state::<super::Runtime>();
    acknowledge_pending(&runtime.flush, session_id, request_id, error);
}
fn acknowledge_pending(
    runtime: &FlushRuntime,
    session_id: u64,
    request_id: u64,
    error: Option<String>,
) {
    let mut pending = runtime.pending.lock();
    if pending
        .as_ref()
        .is_some_and(|(session, request, _)| *session == session_id && *request == request_id)
    {
        if let Some((_, _, sender)) = pending.take() {
            let _ = sender.send(error.map_or(Ok(()), |error| Err(AppError::Config(error))));
        }
    }
}
struct FlushGuard {
    app: AppHandle,
    request_id: u64,
}
impl Drop for FlushGuard {
    fn drop(&mut self) {
        let runtime = self.app.state::<super::Runtime>();
        let mut pending = runtime.flush.pending.lock();
        if pending
            .as_ref()
            .is_some_and(|(_, request, _)| *request == self.request_id)
        {
            pending.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_or_other_session_ack_cannot_release_the_current_barrier() {
        let runtime = FlushRuntime::default();
        let (sender, mut receiver) = oneshot::channel();
        *runtime.pending.lock() = Some((7, 11, sender));
        acknowledge_pending(&runtime, 6, 11, None);
        acknowledge_pending(&runtime, 7, 10, None);
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(runtime.pending.lock().is_some());
        acknowledge_pending(&runtime, 7, 11, None);
        assert!(receiver.try_recv().unwrap().is_ok());
        assert!(runtime.pending.lock().is_none());
    }
    #[test]
    fn persistence_failure_is_returned_instead_of_silently_accepting_old_choices() {
        let runtime = FlushRuntime::default();
        let (sender, mut receiver) = oneshot::channel();
        *runtime.pending.lock() = Some((7, 11, sender));
        acknowledge_pending(&runtime, 7, 11, Some("save failed".into()));
        assert!(receiver.try_recv().unwrap().is_err());
        assert!(runtime.pending.lock().is_none());
    }
}
