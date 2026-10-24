//! Restore WebView2 rendering after suspend without restarting native services.
#[cfg(any(windows, test))]
mod policy;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
use parking_lot::Mutex;
#[cfg(windows)]
use policy::{Policy, Surface, Ticket};
#[cfg(windows)]
use std::{collections::HashMap, sync::Arc, time::Duration};
#[cfg(windows)]
use tauri::Manager;

#[derive(Default)]
pub(crate) struct Runtime {
    #[cfg(windows)]
    policy: Arc<Mutex<Policy>>,
    #[cfg(windows)]
    replacements: Mutex<HashMap<Surface, Restore>>,
    #[cfg(windows)]
    rebuilding: policy::InFlight,
}

pub(crate) fn attach(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        let Some(surface) = Surface::from_label(window.label()) else {
            return;
        };
        let instance = window
            .app_handle()
            .state::<Runtime>()
            .policy
            .lock()
            .attach(surface);
        if windows::attach(window, surface, instance).is_err() {
            tracing::warn!(
                event = "webview_recovery",
                window = surface.label(),
                stage = "hook_failed",
                "could not register display recovery"
            );
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        if let Some(runtime) = app.try_state::<Runtime>() {
            runtime.policy.lock().close();
            runtime.replacements.lock().clear();
        }
        // ExitRequested runs on the owning event-loop thread.
        for window in app.webview_windows().values() {
            windows::detach(window);
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn request(app: tauri::AppHandle, surface: Surface, instance: u64) {
    let Some(policy) = app
        .try_state::<Runtime>()
        .map(|runtime| runtime.policy.clone())
    else {
        return;
    };
    let Some(ticket) = policy.lock().request(surface, instance) else {
        return;
    };
    let Ok(activity) = crate::application::updates::activity::lease() else {
        policy.lock().finish(ticket);
        return;
    };
    tauri::async_runtime::spawn(async move {
        // Notifications can arrive before the display compositor is ready.
        tokio::time::sleep(Duration::from_millis(750)).await;
        if !policy.lock().current(ticket) {
            return;
        }
        tracing::info!(
            event = "webview_recovery",
            window = surface.label(),
            stage = "probe",
            "checking renderer after display resume"
        );
        if windows::probe(&app, ticket, policy.clone()).await {
            policy.lock().finish(ticket);
            tracing::info!(
                event = "webview_recovery",
                window = surface.label(),
                stage = "ready",
                "render surface refreshed with document preserved"
            );
            return;
        }
        // A second chance accommodates transient stalls and frontend startup.
        tokio::time::sleep(Duration::from_secs(2)).await;
        if !policy.lock().current(ticket) {
            return;
        }
        if windows::probe(&app, ticket, policy.clone()).await {
            policy.lock().finish(ticket);
            return;
        }
        let ui_app = app.clone();
        let ui_policy = policy.clone();
        // The queued UI operation retains its fence until it actually ends.
        if app
            .run_on_main_thread(move || {
                if !ui_policy.lock().current(ticket) {
                    return;
                }
                if begin_recreate(&ui_app, ticket, activity).is_err() {
                    tracing::warn!(
                        event = "webview_recovery",
                        window = surface.label(),
                        stage = "recreate_failed",
                        "could not recreate failed renderer"
                    );
                    ui_policy.lock().finish(ticket);
                }
            })
            .is_err()
        {
            policy.lock().finish(ticket);
        }
    });
}

#[cfg(windows)]
fn begin_recreate(
    app: &tauri::AppHandle,
    ticket: Ticket,
    activity: crate::application::updates::activity::ActivityLease<'static>,
) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(ticket.surface.label()) else {
        return Ok(());
    };
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == ticket.surface.label())
        .cloned()
        .ok_or(tauri::Error::WindowNotFound)?;
    let visible = window.is_visible()?;
    let minimized = window.is_minimized()?;
    let maximized = window.is_maximized()?;
    let size = window.inner_size()?;
    let position = window.outer_position()?;
    config.visible = false;
    config.focus = false;
    let restore = Restore {
        ticket,
        config,
        visible,
        minimized,
        maximized,
        size,
        position,
        _activity: activity,
    };
    app.state::<Runtime>()
        .replacements
        .lock()
        .insert(ticket.surface, restore);
    // Destroy skips CloseRequested: do not cancel a pending native operation.
    if let Err(error) = window.destroy() {
        app.state::<Runtime>()
            .replacements
            .lock()
            .remove(&ticket.surface);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
struct Restore {
    ticket: Ticket,
    config: tauri::utils::config::WindowConfig,
    visible: bool,
    minimized: bool,
    maximized: bool,
    size: tauri::PhysicalSize<u32>,
    position: tauri::PhysicalPosition<i32>,
    _activity: crate::application::updates::activity::ActivityLease<'static>,
}

pub(crate) fn is_recovering(app: &tauri::AppHandle) -> bool {
    #[cfg(windows)]
    {
        app.try_state::<Runtime>().is_some_and(|runtime| {
            runtime.rebuilding.active() || !runtime.replacements.lock().is_empty()
        })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        false
    }
}

/// Tauri removes the old label from its manager before delivering this RunEvent.
/// Rebuilding in the destroy() caller would race that cleanup/duplicate the label.
pub(crate) fn destroyed(app: &tauri::AppHandle, label: &str) {
    #[cfg(windows)]
    {
        let Some(surface) = Surface::from_label(label) else {
            return;
        };
        let Some(runtime) = app.try_state::<Runtime>() else {
            return;
        };
        if !runtime.replacements.lock().contains_key(&surface) {
            return;
        }
        let dispatch = app.clone();
        let ui_app = app.clone();
        // Leave the destroy observer before creating another native window;
        // run_on_main_thread would execute inline if called here directly.
        tauri::async_runtime::spawn(async move {
            if dispatch
                .run_on_main_thread(move || complete_recreate(&ui_app, surface))
                .is_err()
            {
                if let Some(runtime) = dispatch.try_state::<Runtime>() {
                    runtime.replacements.lock().remove(&surface);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (app, label);
    }
}

#[cfg(windows)]
fn complete_recreate(app: &tauri::AppHandle, surface: Surface) {
    let Some(runtime) = app.try_state::<Runtime>() else {
        return;
    };
    // Creating WebView2 can pump nested native messages. Keep auto-exit
    // blocked after taking the pending plan and throughout actual creation.
    let _rebuilding = runtime.rebuilding.start();
    let Some(restore) = runtime.replacements.lock().remove(&surface) else {
        return;
    };
    if !runtime.policy.lock().restore_allowed(restore.ticket) {
        return;
    }
    let result = (|| -> tauri::Result<()> {
        let replacement = crate::app_runtime::create_window(app, &restore.config)?;
        if runtime.policy.lock().is_closed() {
            replacement.destroy()?;
            return Ok(());
        }
        replacement.set_size(restore.size)?;
        replacement.set_position(restore.position)?;
        if restore.maximized {
            replacement.maximize()?;
        }
        if restore.visible {
            replacement.show()?;
        }
        if restore.minimized {
            replacement.minimize()?;
        }
        if surface == Surface::Overlay {
            crate::overlay::request_visibility(app);
        }
        Ok(())
    })();
    tracing::warn!(
        event = "webview_recovery",
        window = surface.label(),
        stage = if result.is_ok() {
            "recreated"
        } else {
            "recreate_failed"
        },
        "renderer recovery completed; native services retained"
    );
    runtime.policy.lock().finish(restore.ticket);
}
