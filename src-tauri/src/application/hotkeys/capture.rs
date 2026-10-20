//! Suspend activation during focused renderer button capture. No keys enter IPC.
use super::{register_all_shortcuts, HookRuntime};
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

struct Session {
    _configuration: crate::application::capture_configuration::ConfigurationReservation<'static>,
    _activity: crate::application::updates::activity::ActivityLease<'static>,
}

#[derive(Default)]
pub(crate) struct Runtime {
    current: Mutex<Option<(String, Session)>>,
    closed: AtomicBool,
}

fn take_current<T>(slot: &mut Option<(String, T)>, id: &str) -> Option<T> {
    if slot.as_ref().is_some_and(|(current, _)| current == id) {
        slot.take().map(|(_, session)| session)
    } else {
        None
    }
}

impl Runtime {
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.current.lock().take();
    }
}

pub(crate) fn begin(
    app: AppHandle,
    id: String,
    activity: crate::application::updates::activity::ActivityLease<'static>,
) -> AppResult<String> {
    let runtime = app.state::<Runtime>();
    if runtime.closed.load(Ordering::Acquire) {
        return Err(AppError::Busy("Fono завершается.".into()));
    }
    let configuration = crate::application::capture_configuration::reserve_configuration(|| {
        let pipeline = app.state::<Pipeline>();
        if pipeline.current_operation().is_some()
            || pipeline.has_active_service_operation()
            || crate::application::dictation::workflow::is_busy(&app)
        {
            return Err(AppError::Busy("Сначала завершите текущую диктовку.".into()));
        }
        Ok(())
    })?;
    app.state::<HookRuntime>().stop();
    if app.global_shortcut().unregister_all().is_err() {
        let restored = register_all_shortcuts(&app, &app.state::<AppState>().settings()).is_ok();
        return Err(AppError::Config(if restored {
            "Не удалось начать выбор кнопки. Прежние триггеры восстановлены."
        } else { "Не удалось восстановить триггеры. Сохраните настройки заново или перезапустите Fono." }.into()));
    }
    {
        let mut slot = runtime.current.lock();
        if runtime.closed.load(Ordering::Acquire) {
            return Err(AppError::Busy("Fono завершается.".into()));
        }
        *slot = Some((
            id.clone(),
            Session {
                _configuration: configuration,
                _activity: activity,
            },
        ));
    }
    // Independent watchdog also releases a session whose IPC response/renderer was lost.
    let timeout_id = id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(45)).await;
        let _ = tauri::async_runtime::spawn_blocking(move || end(&app, &timeout_id)).await;
    });
    Ok(id)
}

pub(crate) fn end(app: &AppHandle, id: &str) -> AppResult<()> {
    let runtime = app.state::<Runtime>();
    let Some(_session) = take_current(&mut runtime.current.lock(), id) else {
        return Ok(());
    };
    // The reservation and update lease remain owned through actual restoration.
    if runtime.closed.load(Ordering::Acquire) {
        return Ok(());
    }
    let restored = register_all_shortcuts(app, &app.state::<AppState>().settings());
    if runtime.closed.load(Ordering::Acquire) {
        app.state::<HookRuntime>().stop();
        let _ = app.global_shortcut().unregister_all();
    }
    restored.map_err(|_| {
        AppError::Config(
            "Не удалось восстановить триггеры. Сохраните настройки заново или перезапустите Fono."
                .into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc};
    #[test]
    fn late_timeout_or_cancel_does_not_take_a_successor_session() {
        let mut slot = Some(("new".into(), 7));
        assert_eq!(take_current(&mut slot, "old"), None);
        assert_eq!(take_current(&mut slot, "new"), Some(7));
        assert_eq!(take_current(&mut slot, "new"), None);
    }
    #[test]
    fn taken_guard_survives_detachment_until_restoration_finishes() {
        struct Guard(Arc<AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut slot = Some(("capture".into(), Guard(dropped.clone())));
        let restoring = take_current(&mut slot, "capture");
        assert!(slot.is_none());
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        drop(restoring);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}
