//! Win32 notifications and WebView2 COM objects stay on their owning UI thread.
use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use tauri::{AppHandle, Manager, WebviewWindow};
use webview2_com::{
    CoTaskMemPWSTR, ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
};
use windows::Win32::{
    Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
    System::Power::{
        RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification, HPOWERNOTIFY,
    },
    UI::{
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            DEVICE_NOTIFY_WINDOW_HANDLE, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
            PBT_APMRESUMESUSPEND, WM_DISPLAYCHANGE, WM_NCDESTROY, WM_POWERBROADCAST,
        },
    },
};

use super::{
    policy::{self, Policy, RenderSurface, Surface, Ticket},
    Runtime,
};

const SUBCLASS_ID: usize = 0x464F_4E4F;

struct Hook {
    app: AppHandle,
    surface: Surface,
    instance: u64,
    notification: HPOWERNOTIFY,
}

pub(super) fn attach(window: &WebviewWindow, surface: Surface, instance: u64) -> tauri::Result<()> {
    let hwnd = HWND(window.hwnd()?.0);
    // SAFETY: this HWND is owned by the current Tauri UI thread. The registered
    // subclass owns the Box until detach/WM_NCDESTROY on this same thread.
    unsafe {
        let notification =
            RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE)
                .map_err(|_| tauri::Error::WindowNotFound)?;
        let hook = Box::new(Hook {
            app: window.app_handle().clone(),
            surface,
            instance,
            notification,
        });
        let pointer = Box::into_raw(hook);
        if !SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, pointer as usize).as_bool() {
            let hook = Box::from_raw(pointer);
            let _ = UnregisterSuspendResumeNotification(hook.notification);
            return Err(tauri::Error::WindowNotFound);
        }
    }
    Ok(())
}

pub(super) fn detach(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        // SAFETY: shutdown and creation/destruction run on the HWND owner thread.
        unsafe {
            detach_hwnd(HWND(hwnd.0));
        }
    }
}

unsafe fn detach_hwnd(hwnd: HWND) {
    let mut pointer = 0;
    // SAFETY: access only our own subclass ID/procedure on this UI-thread HWND.
    unsafe {
        if GetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, Some(&mut pointer)).as_bool()
            && RemoveWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID).as_bool()
        {
            let hook = Box::from_raw(pointer as *mut Hook);
            let _ = UnregisterSuspendResumeNotification(hook.notification);
            if let Some(runtime) = hook.app.try_state::<Runtime>() {
                runtime.policy.lock().detach(hook.surface, hook.instance);
            }
        }
    }
}

fn is_resume(message: u32, event: usize) -> bool {
    message == WM_DISPLAYCHANGE
        || (message == WM_POWERBROADCAST
            && matches!(
                event as u32,
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL | PBT_APMRESUMESUSPEND
            ))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: refdata is the Box owned by our subclass until WM_NCDESTROY.
    unsafe {
        if message == WM_NCDESTROY {
            detach_hwnd(hwnd);
        } else if is_resume(message, wparam.0) {
            let hook = &*(data as *const Hook);
            super::request(hook.app.clone(), hook.surface, hook.instance);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

struct Controller(ICoreWebView2Controller);
impl RenderSurface for Controller {
    type Error = webview2_com::Error;

    fn visible(&self) -> Result<bool, Self::Error> {
        let mut visible = Default::default();
        // SAFETY: all COM work occurs inside Tauri's with_webview UI callback.
        unsafe {
            self.0.IsVisible(&mut visible)?;
        }
        Ok(visible.as_bool())
    }

    fn set_visible(&self, visible: bool) -> Result<(), Self::Error> {
        // Controller visibility never shows/activates the native HWND.
        unsafe {
            self.0.SetIsVisible(visible)?;
        }
        Ok(())
    }

    fn refresh_bounds(&self) -> Result<(), Self::Error> {
        let mut bounds = Default::default();
        // Rebind the existing surface without changing document, bounds or focus.
        unsafe {
            self.0.Bounds(&mut bounds)?;
            self.0.SetBounds(bounds)?;
            self.0.NotifyParentWindowPositionChanged()?;
        }
        Ok(())
    }
}

pub(super) async fn probe(app: &AppHandle, ticket: Ticket, policy: Arc<Mutex<Policy>>) -> bool {
    let Some(window) = app.get_webview_window(ticket.surface.label()) else {
        return false;
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let queued = window.with_webview(move |webview| {
        if !policy.lock().current(ticket) {
            return;
        }
        let controller = webview.controller();
        let surface_ready = policy::refresh(&Controller(controller.clone())).is_ok();
        if !surface_ready {
            tracing::warn!(
                event = "webview_recovery",
                window = ticket.surface.label(),
                stage = "surface_error",
                "could not refresh WebView2 surface"
            );
        }
        // Reads only whether Fono mounted. No text, form values or URL is read.
        let script =
            CoTaskMemPWSTR::from("Boolean(document.getElementById('app')?.firstElementChild)");
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, result| {
            let _ = sender.send(surface_ready && status.is_ok() && result == "true");
            Ok(())
        }));
        // SAFETY: controller, script and callback are created/used on the UI thread.
        let submitted = unsafe {
            controller
                .CoreWebView2()
                .and_then(|core| core.ExecuteScript(*script.as_ref().as_pcwstr(), &handler))
        };
        if submitted.is_err() {
            tracing::warn!(
                event = "webview_recovery",
                window = ticket.surface.label(),
                stage = "probe_error",
                "could not submit renderer health check"
            );
        }
    });
    queued.is_ok()
        && matches!(
            tokio::time::timeout(Duration::from_secs(8), receiver).await,
            Ok(Ok(true))
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        PBT_APMPOWERSTATUSCHANGE, PBT_APMSUSPEND, WM_SIZE,
    };

    #[test]
    fn handles_resume_and_display_reconfiguration_but_not_suspend_or_regular_messages() {
        for event in [
            PBT_APMRESUMEAUTOMATIC,
            PBT_APMRESUMECRITICAL,
            PBT_APMRESUMESUSPEND,
        ] {
            assert!(is_resume(WM_POWERBROADCAST, event as usize));
        }
        assert!(is_resume(WM_DISPLAYCHANGE, 0));
        assert!(!is_resume(WM_POWERBROADCAST, PBT_APMSUSPEND as usize));
        assert!(!is_resume(
            WM_POWERBROADCAST,
            PBT_APMPOWERSTATUSCHANGE as usize
        ));
        assert!(!is_resume(WM_SIZE, 0));
    }
}
