//! Explicit opt-in Lira beta. Legacy wake settings remain disabled.
use crate::error::{AppError, AppResult};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[cfg(windows)]
mod discontinuity;
#[cfg(all(windows, any(not(debug_assertions), test)))]
#[path = "../../../build_support/lira.rs"]
mod package;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod supervisor;
#[cfg(windows)]
mod worker;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Phase {
    Off,
    Loading,
    Listening,
    Paused,
    Stopping,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub available: bool,
    pub sample_tools_available: bool,
    pub session_id: Option<u64>,
    pub phase: Phase,
    pub message: String,
}
impl Snapshot {
    fn off() -> Self {
        Self {
            available: available(),
            sample_tools_available: super::wake_samples::testing::available(),
            session_id: None,
            phase: Phase::Off,
            message: String::new(),
        }
    }
    pub fn active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Loading | Phase::Listening | Phase::Paused | Phase::Stopping
        )
    }
}
pub(crate) fn available() -> bool {
    cfg!(windows)
        && if cfg!(debug_assertions) {
            super::wake_samples::testing::available()
        } else {
            option_env!("FONO_LIRA_MANIFEST_SHA256").is_some_and(|hash| hash.len() == 64)
        }
}

#[derive(Default)]
pub(crate) struct Runtime {
    #[cfg(windows)]
    slot: parking_lot::Mutex<Option<Slot>>,
    #[cfg(windows)]
    next: std::sync::atomic::AtomicU64,
}

#[cfg(windows)]
pub(crate) struct Control {
    pub route: parking_lot::Mutex<()>,
    pub cancelled: std::sync::atomic::AtomicBool,
    pub paused: std::sync::atomic::AtomicBool,
    pub revision: std::sync::atomic::AtomicU64,
    pub operation: std::sync::atomic::AtomicU64,
}
#[cfg(windows)]
impl Control {
    fn new() -> Self {
        Self {
            route: parking_lot::Mutex::new(()),
            cancelled: false.into(),
            paused: false.into(),
            revision: 0.into(),
            operation: 0.into(),
        }
    }
    pub fn set_paused(&self, value: bool) {
        use std::sync::atomic::Ordering;
        if self.paused.swap(value, Ordering::AcqRel) != value {
            self.revision.fetch_add(1, Ordering::AcqRel);
        }
    }
}
#[cfg(windows)]
struct Slot {
    id: u64,
    control: std::sync::Arc<Control>,
    status: std::sync::Arc<parking_lot::Mutex<Snapshot>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn snapshot(&self) -> Snapshot {
        #[cfg(windows)]
        if let Some(slot) = self.slot.lock().as_ref() {
            return slot.status.lock().clone();
        }
        Snapshot::off()
    }
    #[cfg(windows)]
    pub fn is_current(&self, id: u64) -> bool {
        self.slot.lock().as_ref().is_some_and(|s| {
            s.id == id
                && !s
                    .control
                    .cancelled
                    .load(std::sync::atomic::Ordering::Acquire)
        })
    }
    pub fn start(&self, app: &AppHandle) -> AppResult<Snapshot> {
        #[cfg(windows)]
        {
            use std::sync::{atomic::Ordering, Arc};
            if !available() {
                return Err(AppError::Config(
                    "Пробуждение недоступно в этой сборке.".into(),
                ));
            }
            let activity = super::updates::activity::background_capture_lease()?;
            let state = app.state::<crate::state::AppState>();
            if state.is_dictation_paused()
                || app
                    .state::<crate::pipeline::Pipeline>()
                    .current_operation()
                    .is_some()
                || super::wake_samples::testing::is_busy()
            {
                return Err(AppError::Busy(
                    "Завершите текущую операцию и снимите паузу диктовки.".into(),
                ));
            }
            let settings = state.settings();
            if settings.selected_stt_model().is_none() {
                return Err(AppError::Config(
                    "Сначала выберите модель распознавания.".into(),
                ));
            }
            let mut slot = self.slot.lock();
            if slot
                .as_ref()
                .is_some_and(|s| s.thread.as_ref().map_or(true, |t| !t.is_finished()))
            {
                return Err(AppError::Busy(
                    "Пробуждение уже включено или выключается.".into(),
                ));
            }
            if let Some(mut old) = slot.take() {
                old.control.cancelled.store(true, Ordering::Release);
                if let Some(thread) = old.thread.take() {
                    let _ = thread.join();
                }
            }
            let id = self.next.fetch_add(1, Ordering::AcqRel) + 1;
            let worker_reservation = super::wake_samples::testing::reserve_worker()?;
            let control = Arc::new(Control::new());
            let snapshot = Snapshot {
                available: true,
                sample_tools_available: super::wake_samples::testing::available(),
                session_id: Some(id),
                phase: Phase::Loading,
                message: String::new(),
            };
            let status = Arc::new(parking_lot::Mutex::new(snapshot.clone()));
            let (handle, owner, view) = (app.clone(), Arc::clone(&control), Arc::clone(&status));
            let thread = std::thread::Builder::new()
                .name("lira-live".into())
                .spawn(move || {
                    let _activity = activity;
                    let _worker_reservation = worker_reservation;
                    let mut worker = None;
                    let result = supervisor::run(
                        || {
                            let result = service::run(&handle, id, &owner, &view,
                                settings.audio_device_id.clone(), &mut worker);
                            // A broken microphone must not cold-load healthy models.
                            // A failed pipe/worker cannot be safely reused.
                            if matches!(&result, Err(AppError::Internal(_))) {
                                drop(worker.take());
                            }
                            result
                        },
                        || owner.cancelled.load(Ordering::Acquire),
                        |restart, error| {
                            tracing::warn!(
                                event = "lira_recovering",
                                listener_session = id,
                                restart,
                                error_code = ?error.user_error().code,
                                "live wake detector is restarting after failure"
                            );
                            {
                                let mut status = view.lock();
                                if owner.cancelled.load(Ordering::Acquire) || status.phase == Phase::Stopping {
                                    return false;
                                }
                                status.phase = Phase::Loading;
                                status.message.clear();
                            }
                            crate::tray::request_refresh(&handle);
                            // A bounded backoff stays interruptible by the owner's Stop.
                            for _ in 0..10 * restart {
                                if owner.cancelled.load(Ordering::Acquire) {
                                    return false;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                            true
                        },
                    );
                    let was_cancelled = owner.cancelled.load(Ordering::Acquire);
                    let operation = {
                        let _route = owner.route.lock();
                        owner.cancelled.store(true, Ordering::Release);
                        owner.operation.load(Ordering::Acquire)
                    };
                    if operation != 0 {
                        let app = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ =
                                crate::ipc::dictation::cancel_dictation(app, Some(operation)).await;
                        });
                    }
                    let mut status = view.lock();
                    if was_cancelled {
                        tracing::info!(event = "lira_stopped", listener_session = id, "live wake explicitly stopped");
                        status.phase = Phase::Off;
                        status.message.clear();
                    } else {
                        tracing::warn!(event = "lira_failed", listener_session = id,
                            error_code = ?result.as_ref().err().map(|error| error.user_error().code),
                            "live wake stopped after failure");
                        status.phase = Phase::Error;
                        status.message =
                            result.err().map(|e| match e {
                                AppError::Internal(_) => "Детектор пробуждения остановился после повторных попыток. Включите пробуждение снова.".into(),
                                _ => e.user_message(),
                            }).unwrap_or_else(|| {
                                "Пробуждение остановлено. Включите его снова.".into()
                            });
                    }
                    drop(status);
                    crate::tray::request_refresh(&handle);
                })
                .map_err(|_| AppError::Internal("Не удалось включить пробуждение.".into()))?;
            *slot = Some(Slot {
                id,
                control,
                status,
                thread: Some(thread),
            });
            crate::tray::request_refresh(app);
            Ok(snapshot)
        }
        #[cfg(not(windows))]
        {
            let _ = app;
            Err(AppError::Config(
                "Пробуждение недоступно в этой сборке.".into(),
            ))
        }
    }
    pub fn request_stop(&self, id: u64, app: &AppHandle) -> Option<u64> {
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            let (control, status) = {
                let slot = self.slot.lock();
                let slot = slot.as_ref().filter(|s| s.id == id)?;
                (
                    std::sync::Arc::clone(&slot.control),
                    std::sync::Arc::clone(&slot.status),
                )
            };
            let _route = control.route.lock();
            control.cancelled.store(true, Ordering::Release);
            control.revision.fetch_add(1, Ordering::AcqRel);
            status.lock().phase = Phase::Stopping;
            let operation = control.operation.load(Ordering::Acquire);
            if let Some(token) = app
                .state::<crate::pipeline::Pipeline>()
                .cancellation(operation)
            {
                token.cancel();
            }
            (operation != 0).then_some(operation)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, app);
            None
        }
    }
    pub fn join(&self, id: u64) {
        #[cfg(windows)]
        {
            let thread = self
                .slot
                .lock()
                .as_mut()
                .filter(|s| s.id == id)
                .and_then(|s| s.thread.take());
            if let Some(thread) = thread {
                let _ = thread.join();
                let mut slot = self.slot.lock();
                if slot
                    .as_ref()
                    .is_some_and(|s| s.id == id && s.thread.is_none())
                {
                    slot.take();
                }
            }
        }
        #[cfg(not(windows))]
        let _ = id;
    }
    fn suspend(&self, value: bool) {
        #[cfg(windows)]
        if let Some(slot) = self.slot.lock().as_ref() {
            slot.control.set_paused(value);
        }
        #[cfg(not(windows))]
        let _ = value;
    }
}
pub(crate) fn pause(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<Runtime>() {
        runtime.suspend(true);
    }
}
pub(crate) fn resume(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<Runtime>() {
        runtime.suspend(false);
    }
}
pub(crate) fn shutdown(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<Runtime>() {
        if let Some(id) = runtime.snapshot().session_id {
            runtime.request_stop(id, app);
            runtime.join(id);
        }
    }
}
pub(crate) async fn stop(app: AppHandle, id: u64) -> AppResult<Snapshot> {
    let operation = app.state::<Runtime>().request_stop(id, &app);
    if let Some(operation) = operation {
        let _ = crate::ipc::dictation::cancel_dictation(app.clone(), Some(operation)).await;
    }
    let activity = super::updates::activity::cleanup_lease();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        handle.state::<Runtime>().join(id);
    })
    .await
    .map_err(|_| AppError::Internal("Не удалось завершить пробуждение.".into()))?;
    crate::tray::request_refresh(&app);
    Ok(app.state::<Runtime>().snapshot())
}

#[cfg(any(windows, test))]
pub(crate) fn strip_phrase(text: &str) -> String {
    static PHRASE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let phrase = PHRASE.get_or_init(|| {
        // Dictation STT can spell the already-confirmed name differently from
        // the separate wake verifier. This is cleanup only, never acceptance.
        regex::Regex::new(r"(?i)\bпривет[\s,.:!?;–—-]+(?:лира|лера|ира)\b[\s,.:!?;–—-]*")
            .expect("fixed phrase")
    });
    phrase.find(text).map_or_else(
        || text.trim().to_owned(),
        |found| text[found.end()..].trim().to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_wake_phrase_and_its_preroll_are_removed() {
        assert_eq!(
            strip_phrase("Старый разговор. Привет, Лира! Запиши заметку."),
            "Запиши заметку."
        );
        assert_eq!(strip_phrase("Привет Лира"), "");
        assert_eq!(
            strip_phrase("Привет, Лиран. Запиши"),
            "Привет, Лиран. Запиши"
        );
        assert_eq!(strip_phrase("Расскажи про iPhone"), "Расскажи про iPhone");
    }
    #[test]
    fn confirmed_wake_cleans_dictation_stt_name_variants_once_without_cutting_the_command() {
        for name in ["Лира", "Лера", "Ира"] {
            assert_eq!(
                strip_phrase(&format!("Привет, {name}! Первое слово на месте.")),
                "Первое слово на месте."
            );
            assert_eq!(strip_phrase(&format!("ПРИВЕТ – {name}.")), "");
            assert_eq!(
                strip_phrase(&format!("Привет {name}. Напиши: привет, Ира!")),
                "Напиши: привет, Ира!"
            );
        }
        for text in [
            "Привет, Лиран. Запиши",
            "Привет, Иран",
            "Привет, Мира",
            "Лира звучит красиво",
        ] {
            assert_eq!(strip_phrase(text), text);
        }
    }
}
