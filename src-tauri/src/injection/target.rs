//! Focus identity without reading field contents. UI Automation lives on one
//! bounded COM owner; a slow provider pauses insertion instead of spawning threads.
use crate::{
    error::{AppError, AppResult},
    types::InjectionMode,
};
use fono_core::OperationCancellation;
mod executable;
pub(crate) use executable::for_target as executable_name;
pub(crate) type InputPreflight = std::sync::Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextTarget {
    pub window: isize,
    pub focus: isize,
    pub process: u32,
    pub runtime_id: Vec<i32>,
}

#[derive(Debug)]
pub struct AppendResult {
    pub bytes: usize,
    pub paused: bool,
    pub uncertain: bool,
}

pub fn capture() -> AppResult<TextTarget> {
    platform::capture()?.ok_or_else(|| {
        AppError::Injection(
            "Выберите доступное текстовое поле в другом приложении и нажмите горячую клавишу"
                .into(),
        )
    })
}
pub fn matches(target: &TextTarget) -> bool {
    // An unavailable probe always blocks insertion; it must never authorize a
    // retry using stale focus identity.
    platform::capture().ok().flatten().as_ref() == Some(target)
}

/// A cheap final race check after the full UIA identity check. This alone never
/// authorizes a paste, because two fields can share the same native handle.
#[cfg(windows)]
pub(super) fn has_native_focus(target: &TextTarget) -> bool {
    platform::has_native_focus(target)
}

#[cfg(windows)]
pub(super) unsafe fn selection_target(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Option<(
    TextTarget,
    windows::Win32::UI::Accessibility::IUIAutomationElement,
)> {
    platform::query_element(automation)
}

pub fn append(
    target: &TextTarget,
    text: &str,
    mode: InjectionMode,
    cancel: &OperationCancellation,
) -> AppResult<AppendResult> {
    append_with_preflight(target, text, mode, cancel, None)
}

pub(crate) fn append_with_preflight(
    target: &TextTarget,
    text: &str,
    mode: InjectionMode,
    cancel: &OperationCancellation,
    preflight: Option<InputPreflight>,
) -> AppResult<AppendResult> {
    if cancel.is_cancelled() {
        return Ok(AppendResult {
            bytes: 0,
            paused: true,
            uncertain: false,
        });
    }
    match mode {
        // Both backends check the complete field identity at their send boundary.
        // Do not add another provider round trip before the clipboard owner.
        InjectionMode::SendInput => platform::append(target, text, cancel, preflight.as_deref()),
        InjectionMode::Clipboard => {
            super::inject_via_clipboard_preflight(text, target.clone(), cancel.clone(), preflight)?;
            Ok(AppendResult {
                bytes: text.len(),
                paused: false,
                uncertain: false,
            })
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn capture() -> AppResult<Option<TextTarget>> {
        Ok(None)
    }
    pub fn append(
        _: &TextTarget,
        _: &str,
        _: &OperationCancellation,
        _: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> AppResult<AppendResult> {
        Err(AppError::Injection(
            "Живая вставка доступна в Windows".into(),
        ))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crossbeam_channel::{bounded, Sender};
    use once_cell::sync::Lazy;
    use std::time::{Duration, Instant};
    use windows::Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        },
        System::Ole::{
            SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationValuePattern,
            TreeScope_Element, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
            UIA_EditControlTypeId, UIA_HasKeyboardFocusPropertyId, UIA_IsEnabledPropertyId,
            UIA_IsPasswordPropertyId, UIA_IsTextEditPatternAvailablePropertyId,
            UIA_ProcessIdPropertyId, UIA_RuntimeIdPropertyId, UIA_ValueIsReadOnlyPropertyId,
            UIA_ValuePatternId,
        },
        UI::Input::KeyboardAndMouse::{SendInput, INPUT},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
        },
    };
    const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct NativeFocus {
        window: isize,
        focus: isize,
        process: u32,
    }
    impl NativeFocus {
        unsafe fn capture() -> Option<Self> {
            let window = GetForegroundWindow();
            if window.0.is_null() {
                return None;
            }
            let mut process = 0;
            let thread = GetWindowThreadProcessId(window, Some(&mut process));
            if thread == 0 || process == 0 || process == std::process::id() {
                return None;
            }
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(thread, &mut info).ok()?;
            if GetForegroundWindow() != window {
                return None;
            }
            Some(Self {
                window: window.0 as isize,
                focus: info.hwndFocus.0 as isize,
                process,
            })
        }

        fn matches(self, target: &TextTarget) -> bool {
            self.window == target.window
                && self.focus == target.focus
                && self.process == target.process
        }
    }

    struct ProbeRequest {
        native: NativeFocus,
        deadline: Instant,
        reply: Sender<Option<TextTarget>>,
    }

    pub(super) fn has_native_focus(target: &TextTarget) -> bool {
        unsafe { NativeFocus::capture() }.is_some_and(|focus| focus.matches(target))
    }

    static PROBE: Lazy<std::io::Result<Sender<ProbeRequest>>> = Lazy::new(|| {
        let (tx, rx) = bounded::<ProbeRequest>(1);
        std::thread::Builder::new()
            .name("fono-focus-owner".into())
            .spawn(move || unsafe {
                let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
                let automation: Option<IUIAutomation> = if initialized {
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
                } else {
                    None
                };
                let cache = automation.as_ref().and_then(|a| cache_request(a).ok());
                while let Ok(request) = rx.recv() {
                    let target = answer_probe(&request, || {
                        query_cached(automation.as_ref()?, cache.as_ref()?)
                            .map(|(target, _)| target)
                    });
                    let _ = request.reply.send(target);
                }
                drop(cache);
                drop(automation);
                if initialized {
                    windows::Win32::System::Com::CoUninitialize();
                }
            })?;
        Ok(tx)
    });

    fn answer_probe(
        request: &ProbeRequest,
        query: impl FnOnce() -> Option<TextTarget>,
    ) -> Option<TextTarget> {
        if Instant::now() >= request.deadline {
            return None;
        }
        let target = query()?;
        // A late provider reply must never authorize a different foreground or
        // a subsequent operation which happened to enqueue another probe.
        (Instant::now() < request.deadline && request.native.matches(&target)).then_some(target)
    }

    pub fn capture() -> AppResult<Option<TextTarget>> {
        let Some(native) = (unsafe { NativeFocus::capture() }) else {
            return Ok(None);
        };
        let deadline = Instant::now() + PROBE_TIMEOUT;
        let probe = PROBE.as_ref().map_err(|_| {
            AppError::Injection(
                "Не удалось запустить проверку поля ввода. Перезапустите Fono".into(),
            )
        })?;
        let (tx, rx) = bounded(1);
        if probe
            .try_send(ProbeRequest {
                native,
                deadline,
                reply: tx,
            })
            .is_err()
        {
            return Ok(None);
        }
        Ok(rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
            .flatten())
    }

    unsafe fn cache_request(
        automation: &IUIAutomation,
    ) -> windows::core::Result<IUIAutomationCacheRequest> {
        let cache = automation.CreateCacheRequest()?;
        cache.SetTreeScope(TreeScope_Element)?;
        for property in [
            UIA_ProcessIdPropertyId,
            UIA_ControlTypePropertyId,
            UIA_IsEnabledPropertyId,
            UIA_IsPasswordPropertyId,
            UIA_HasKeyboardFocusPropertyId,
            UIA_RuntimeIdPropertyId,
            UIA_ValueIsReadOnlyPropertyId,
            UIA_IsTextEditPatternAvailablePropertyId,
        ] {
            cache.AddProperty(property)?;
        }
        cache.AddPattern(UIA_ValuePatternId)?;
        Ok(cache)
    }

    pub(super) unsafe fn query_element(
        automation: &IUIAutomation,
    ) -> Option<(
        TextTarget,
        windows::Win32::UI::Accessibility::IUIAutomationElement,
    )> {
        query_cached(automation, &cache_request(automation).ok()?)
    }

    unsafe fn query_cached(
        automation: &IUIAutomation,
        cache: &IUIAutomationCacheRequest,
    ) -> Option<(
        TextTarget,
        windows::Win32::UI::Accessibility::IUIAutomationElement,
    )> {
        let native = NativeFocus::capture()?;
        // One fresh provider snapshot per check, rather than separate remote
        // calls for every property. Cached here never means a previous check.
        let element = automation.GetFocusedElementBuildCache(cache).ok()?;
        if element.CachedProcessId().ok()? as u32 != native.process
            || !element.CachedHasKeyboardFocus().ok()?.as_bool()
            || !element.CachedIsEnabled().ok()?.as_bool()
            || element.CachedIsPassword().ok()?.as_bool()
        {
            return None;
        }
        let kind = element.CachedControlType().ok()?;
        if kind != UIA_EditControlTypeId && kind != UIA_DocumentControlTypeId {
            return None;
        }
        let editable = element
            .GetCachedPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .and_then(|pattern| pattern.CachedIsReadOnly().ok())
            .is_some_and(|readonly| !readonly.as_bool())
            || element
                .GetCachedPropertyValue(UIA_IsTextEditPatternAvailablePropertyId)
                .ok()
                .and_then(|value| bool::try_from(&value).ok())
                .unwrap_or(false);
        if !editable {
            return None;
        }
        let array = element.GetRuntimeId().ok()?;
        if array.is_null() {
            return None;
        }
        let result = (|| {
            let first = SafeArrayGetLBound(array, 1).ok()?;
            let last = SafeArrayGetUBound(array, 1).ok()?;
            if last < first || last - first > 64 {
                return None;
            }
            let mut runtime_id = Vec::new();
            for index in first..=last {
                let mut value = 0i32;
                SafeArrayGetElement(array, &index, (&mut value as *mut i32).cast()).ok()?;
                runtime_id.push(value);
            }
            if runtime_id.is_empty() || NativeFocus::capture() != Some(native) {
                return None;
            }
            Some((
                TextTarget {
                    window: native.window,
                    focus: native.focus,
                    process: native.process,
                    runtime_id,
                },
                element,
            ))
        })();
        let _ = SafeArrayDestroy(array);
        result
    }

    pub fn append(
        target: &TextTarget,
        text: &str,
        cancel: &OperationCancellation,
        preflight: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> AppResult<AppendResult> {
        let mut bytes = 0;
        let mut chars = text.char_indices().peekable();
        while chars.peek().is_some() {
            super::super::modifiers::wait_for_release(|| cancel.is_cancelled())?;
            if cancel.is_cancelled() || !super::matches(target) {
                return Ok(AppendResult {
                    bytes,
                    paused: true,
                    uncertain: false,
                });
            }
            // Selection content is read only for an explicitly confirmed replacement,
            // once before the first batch. Later batches retain the metadata guard.
            if let Some(check) = preflight.filter(|_| bytes == 0) {
                if !check() || cancel.is_cancelled() || !super::matches(target) {
                    return Ok(AppendResult {
                        bytes: 0,
                        paused: true,
                        uncertain: false,
                    });
                }
                super::super::modifiers::ensure_released()?;
            }
            let batch: Vec<_> = chars.by_ref().take(8).collect();
            let inputs: Vec<_> = batch
                .iter()
                .flat_map(|(_, c)| {
                    c.to_string()
                        .encode_utf16()
                        .flat_map(super::super::build_unicode_inputs)
                        .collect::<Vec<_>>()
                })
                .collect();
            let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
            if sent != inputs.len() as u32 {
                super::super::release_unicode_units(
                    &batch.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
                );
                return Ok(AppendResult {
                    bytes,
                    paused: false,
                    uncertain: true,
                });
            }
            if let Some((offset, c)) = batch.last() {
                bytes = offset + c.len_utf8();
            }
        }
        Ok(AppendResult {
            bytes,
            paused: false,
            uncertain: false,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;

        fn request(deadline: Instant) -> ProbeRequest {
            ProbeRequest {
                native: NativeFocus {
                    window: 1,
                    focus: 2,
                    process: 3,
                },
                deadline,
                reply: bounded(1).0,
            }
        }
        fn target() -> TextTarget {
            TextTarget {
                window: 1,
                focus: 2,
                process: 3,
                runtime_id: vec![4, 5],
            }
        }

        #[test]
        fn expired_queued_probes_do_not_call_a_slow_provider_again() {
            let calls = Cell::new(0);
            let result = answer_probe(&request(Instant::now()), || {
                calls.set(calls.get() + 1);
                Some(target())
            });
            assert!(result.is_none());
            assert_eq!(calls.get(), 0);
        }

        #[test]
        fn provider_reply_for_another_native_field_is_rejected() {
            let request = request(Instant::now() + Duration::from_secs(5));
            assert!(answer_probe(&request, || Some(target())).is_some());
            for target in [
                TextTarget {
                    window: 9,
                    ..target()
                },
                TextTarget {
                    focus: 9,
                    ..target()
                },
                TextTarget {
                    process: 9,
                    ..target()
                },
            ] {
                assert!(answer_probe(&request, || Some(target)).is_none());
            }
            assert!(answer_probe(&request, || None).is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fields_in_the_same_window_have_distinct_identity() {
        let first = TextTarget {
            window: 1,
            focus: 2,
            process: 3,
            runtime_id: vec![7, 8],
        };
        let second = TextTarget {
            runtime_id: vec![7, 9],
            ..first.clone()
        };
        assert_ne!(first, second);
    }
}
