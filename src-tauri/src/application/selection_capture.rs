//! Separate explicit selection lifecycle; no audio, LLM, history or replacement.
use super::updates::activity::{self, ActivityLease};
use crate::{
    error::{AppError, AppResult},
    injection::selection::{self, CapturedSelection, SelectionSource},
    pipeline::Pipeline,
    state::AppState,
};
use fono_core::OperationCancellation;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Serialize)]
pub(crate) struct Snapshot {
    pub operation_id: u64,
    pub phase: &'static str,
    pub text: Option<String>,
    pub source: Option<SelectionSource>,
    pub message: Option<String>,
}
struct Active {
    id: u64,
    cancel: OperationCancellation,
    result: Option<AppResult<CapturedSelection>>,
    _lease: ActivityLease<'static>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.cancel.cancel();
        selection::release(self.id);
    }
}
#[derive(Default)]
struct Slot {
    closed: bool,
    active: Option<Active>,
}
#[derive(Default)]
pub(crate) struct Runtime {
    slot: Mutex<Slot>,
    next: AtomicU64,
}

impl Runtime {
    pub(crate) fn snapshot(&self) -> Option<Snapshot> {
        let slot = self.slot.lock();
        let a = slot.active.as_ref()?;
        let (phase, text, source, message) = match &a.result {
            None => ("reading", None, None, None),
            Some(Ok(value)) => ("ready", Some(value.text.clone()), Some(value.source), None),
            Some(Err(e)) => ("error", None, None, Some(e.user_error().params.detail.unwrap_or_else(|| "Не удалось безопасно получить выделение. Повторите действие или выберите другое поле.".into()))),
        };
        Some(Snapshot {
            operation_id: a.id,
            phase,
            text,
            source,
            message,
        })
    }
    pub(crate) fn cancel(&self, id: u64) -> bool {
        let mut slot = self.slot.lock();
        if slot.active.as_ref().is_some_and(|a| a.id == id) {
            slot.active.take();
            true
        } else {
            false
        }
    }
    pub(crate) fn close(&self) {
        let mut slot = self.slot.lock();
        slot.closed = true;
        slot.active.take();
    }
    fn begin(&self, lease: ActivityLease<'static>) -> AppResult<(u64, OperationCancellation)> {
        let mut slot = self.slot.lock();
        if slot.closed || slot.active.is_some() {
            return Err(AppError::Busy(
                "Предыдущее получение выделения ещё завершается.".into(),
            ));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = OperationCancellation::default();
        slot.active = Some(Active {
            id,
            cancel: cancel.clone(),
            result: None,
            _lease: lease,
        });
        Ok((id, cancel))
    }
    fn complete(&self, id: u64, result: AppResult<CapturedSelection>) -> bool {
        let result = result.and_then(|selection| {
            if selection.belongs_to(id) {
                Ok(selection)
            } else {
                Err(AppError::Injection(
                    "Поле или выделение изменилось. Выделите фрагмент заново.".into(),
                ))
            }
        });
        let mut slot = self.slot.lock();
        let Some(a) = slot
            .active
            .as_mut()
            .filter(|a| a.id == id && !a.cancel.is_cancelled())
        else {
            return false;
        };
        a.result = Some(result);
        true
    }
}

fn notify(app: &AppHandle) {
    let _ = app.emit_to("settings", "selection-capture-changed", ());
}
pub(crate) fn cancel(app: &AppHandle, id: u64) {
    if app.state::<Runtime>().cancel(id) {
        notify(app);
    }
}

pub(crate) fn request(app: AppHandle) {
    let settings = app.state::<AppState>().settings();
    if !settings.selection_capture_enabled {
        return;
    }
    let runtime = app.state::<Runtime>();
    if let Some(active) = runtime.snapshot() {
        cancel(&app, active.operation_id);
        return;
    }
    let Ok(work_lease) = activity::lease() else {
        return;
    };
    // Validate/reserve before queueing. A concurrent dictation or settings writer
    // cannot change the source while this explicit read is being prepared.
    let configuration = super::capture_configuration::try_reserve_configuration(|| {
        let pipeline = app.state::<Pipeline>();
        if pipeline.current_operation().is_some()
            || pipeline.has_active_service_operation()
            || super::dictation::workflow::is_busy(&app)
        {
            return Err(AppError::Busy("Сначала завершите текущую диктовку.".into()));
        }
        Ok(())
    });
    let Ok(configuration) = configuration else {
        return;
    };
    let Ok((id, cancellation)) = runtime.begin(work_lease.clone()) else {
        return;
    };
    notify(&app);
    let fallback = settings.selection_clipboard_fallback;
    tauri::async_runtime::spawn_blocking(move || {
        let _configuration = configuration;
        let _work_lease = work_lease;
        let result = selection::capture(id, cancellation.clone(), fallback, _work_lease.clone());
        if !app.state::<Runtime>().complete(id, result) {
            return;
        }
        notify(&app);
        // Foreground is changed only after the explicit read and clipboard cleanup.
        let ui_app = app.clone();
        let ui_lease = _work_lease.clone();
        let _ = app.run_on_main_thread(move || {
            let _lease = ui_lease;
            if ui_app
                .state::<Runtime>()
                .snapshot()
                .is_some_and(|s| s.operation_id == id)
            {
                if let Some(window) = ui_app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::injection::{
        selection::{captured, SelectionAnchor},
        target::TextTarget,
    };
    fn fixture(id: u64) -> CapturedSelection {
        captured(
            "PRIVATE SELECTED TEXT".into(),
            SelectionSource::UiAutomation,
            TextTarget {
                window: 1,
                focus: 2,
                process: 3,
                runtime_id: vec![4],
            },
            SelectionAnchor::UiAutomation { operation: id },
        )
        .unwrap()
    }
    #[test]
    fn cancelled_read_cannot_publish_text_into_a_successor() {
        let runtime = Runtime::default();
        let (old, cancelled) = runtime.begin(activity::lease().unwrap()).unwrap();
        assert!(runtime.cancel(old));
        assert!(cancelled.is_cancelled());
        let (next, _) = runtime.begin(activity::lease().unwrap()).unwrap();
        assert!(!runtime.complete(old, Ok(fixture(old))));
        assert!(!runtime.cancel(old));
        assert!(runtime.complete(next, Ok(fixture(next))));
        assert_eq!(
            runtime.snapshot().unwrap().text.as_deref(),
            Some("PRIVATE SELECTED TEXT")
        );
        assert!(runtime.cancel(next));
        assert!(runtime.snapshot().is_none());
    }
    #[test]
    fn snapshots_expose_only_authorized_text_and_source_not_target_or_proof() {
        let runtime = Runtime::default();
        let (id, _) = runtime.begin(activity::lease().unwrap()).unwrap();
        assert!(runtime.snapshot().unwrap().text.is_none());
        assert!(runtime.complete(id, Ok(fixture(id))));
        let json = serde_json::to_value(runtime.snapshot().unwrap()).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 5);
        assert!(json.get("target").is_none());
        assert!(json.get("digest").is_none());
        runtime.close();
        assert!(runtime.snapshot().is_none());
        assert!(runtime.begin(activity::lease().unwrap()).is_err());
    }
    #[test]
    fn unknown_errors_never_publish_paths_or_partial_text() {
        let runtime = Runtime::default();
        let (id, _) = runtime.begin(activity::lease().unwrap()).unwrap();
        runtime.complete(id, Err(AppError::Injection("PRIVATE PATH OR ERROR".into())));
        let snapshot = runtime.snapshot().unwrap();
        assert_eq!(snapshot.phase, "error");
        assert!(snapshot.text.is_none());
        assert!(!snapshot.message.unwrap().contains("PRIVATE"));
    }
}
