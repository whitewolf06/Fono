//! Focus identity without reading field contents. UI Automation lives on one
//! bounded COM owner; a slow provider pauses insertion instead of spawning threads.
use crate::{
    error::{AppError, AppResult},
    operation::OperationCancellation,
    types::InjectionMode,
};

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
    platform::capture().ok_or_else(|| {
        AppError::Injection(
            "Выберите доступное текстовое поле в другом приложении и нажмите горячую клавишу"
                .into(),
        )
    })
}
pub fn matches(target: &TextTarget) -> bool {
    platform::capture().as_ref() == Some(target)
}

pub fn append(
    target: &TextTarget,
    text: &str,
    mode: InjectionMode,
    cancel: &OperationCancellation,
) -> AppResult<AppendResult> {
    if cancel.is_cancelled() || !matches(target) {
        return Ok(AppendResult {
            bytes: 0,
            paused: true,
            uncertain: false,
        });
    }
    match mode {
        InjectionMode::SendInput => platform::append(target, text, cancel),
        InjectionMode::Clipboard => {
            super::inject_via_clipboard_checked(text, target.clone(), cancel.clone())?;
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
    pub fn capture() -> Option<TextTarget> {
        None
    }
    pub fn append(_: &TextTarget, _: &str, _: &OperationCancellation) -> AppResult<AppendResult> {
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
    use std::time::Duration;
    use windows::Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        },
        System::Ole::{
            SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationTextEditPattern, IUIAutomationValuePattern,
            UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_TextEditPatternId,
            UIA_ValuePatternId,
        },
        UI::Input::KeyboardAndMouse::{SendInput, INPUT},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
        },
    };
    static PROBE: Lazy<Sender<Sender<Option<TextTarget>>>> = Lazy::new(|| {
        let (tx, rx) = bounded::<Sender<Option<TextTarget>>>(1);
        std::thread::Builder::new()
            .name("fono-focus-owner".into())
            .spawn(move || unsafe {
                let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
                let automation: Option<IUIAutomation> = if initialized {
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
                } else {
                    None
                };
                while let Ok(reply) = rx.recv() {
                    let _ =
                        reply.send(automation.as_ref().and_then(|automation| query(automation)));
                }
                if initialized {
                    windows::Win32::System::Com::CoUninitialize();
                }
            })
            .expect("focus owner thread");
        tx
    });

    pub fn capture() -> Option<TextTarget> {
        let (tx, rx) = bounded(1);
        PROBE.try_send(tx).ok()?;
        rx.recv_timeout(Duration::from_millis(100)).ok().flatten()
    }

    unsafe fn query(automation: &IUIAutomation) -> Option<TextTarget> {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let mut process = 0;
        let thread = GetWindowThreadProcessId(window, Some(&mut process));
        if process == std::process::id() {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(thread, &mut info).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        if element.CurrentProcessId().ok()? as u32 != process
            || !element.CurrentIsEnabled().ok()?.as_bool()
        {
            return None;
        }
        let kind = element.CurrentControlType().ok()?;
        if kind != UIA_EditControlTypeId && kind != UIA_DocumentControlTypeId {
            return None;
        }
        let editable = element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .and_then(|pattern| pattern.CurrentIsReadOnly().ok())
            .is_some_and(|readonly| !readonly.as_bool())
            || element
                .GetCurrentPatternAs::<IUIAutomationTextEditPattern>(UIA_TextEditPatternId)
                .is_ok();
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
            if runtime_id.is_empty() || GetForegroundWindow() != window {
                return None;
            }
            Some(TextTarget {
                window: window.0 as isize,
                focus: info.hwndFocus.0 as isize,
                process,
                runtime_id,
            })
        })();
        let _ = SafeArrayDestroy(array);
        result
    }

    pub fn append(
        target: &TextTarget,
        text: &str,
        cancel: &OperationCancellation,
    ) -> AppResult<AppendResult> {
        let mut bytes = 0;
        let mut chars = text.char_indices().peekable();
        while chars.peek().is_some() {
            if cancel.is_cancelled() || !super::matches(target) {
                return Ok(AppendResult {
                    bytes,
                    paused: true,
                    uncertain: false,
                });
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
