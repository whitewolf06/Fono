use super::*;
use crate::application::updates::activity::ActivityLease;
use ::windows::{
    core::Interface,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationTextPattern,
                IUIAutomationTextRange, TextPatternRangeEndpoint_End,
                TextPatternRangeEndpoint_Start, UIA_TextPatternId, UIA_E_NOTSUPPORTED,
            },
            WindowsAndMessaging::{GetClassNameW, SendMessageTimeoutW, SMTO_ABORTIFHUNG},
        },
    },
};
use crossbeam_channel::{bounded, Sender};
use once_cell::sync::Lazy;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

enum Command {
    Verify {
        operation: u64,
        original: CapturedSelection,
        cancel: OperationCancellation,
        _lease: ActivityLease<'static>,
        reply: Sender<bool>,
    },
    Read {
        operation: u64,
        cancel: OperationCancellation,
        fallback: bool,
        _lease: ActivityLease<'static>,
        reply: Sender<AppResult<CapturedSelection>>,
    },
    Release {
        operation: u64,
        _lease: ActivityLease<'static>,
    },
}
struct Worker {
    tx: Sender<Command>,
    closed: Arc<AtomicBool>,
}
static WORKER: Lazy<std::io::Result<Worker>> = Lazy::new(|| {
    let (tx, rx) = bounded(1);
    let closed = Arc::new(AtomicBool::new(false));
    let closing = closed.clone();
    std::thread::Builder::new()
        .name("fono-selection-owner".into())
        .spawn(move || unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let automation: Option<IUIAutomation> = if initialized {
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()
            } else {
                None
            };
            if let Some(a) = automation
                .as_ref()
                .and_then(|a| a.cast::<IUIAutomation2>().ok())
            {
                let _ = a.SetConnectionTimeout(500);
                let _ = a.SetTransactionTimeout(800);
            }
            // COM ranges are retained and destroyed exclusively by this owner.
            let mut retained: Option<(u64, IUIAutomationTextRange)> = None;
            let mut cleanup = None;
            while let Ok(command) = rx.recv() {
                if closing.load(Ordering::Acquire) {
                    cleanup = Some(crate::application::updates::activity::cleanup_lease());
                    break;
                }
                match command {
                    Command::Verify {
                        operation,
                        original,
                        cancel,
                        _lease,
                        reply,
                    } => {
                        let valid = !cancel.is_cancelled()
                            && automation.as_ref().is_some_and(|a| {
                                verify_range(a, operation, &original, retained.as_ref())
                            });
                        let _ = reply.send(valid && !cancel.is_cancelled());
                    }
                    Command::Release {
                        operation: id,
                        _lease,
                    } => {
                        if retained
                            .as_ref()
                            .is_some_and(|(operation, _)| *operation == id)
                        {
                            retained = None;
                        }
                    }
                    Command::Read {
                        operation,
                        cancel,
                        fallback,
                        _lease,
                        reply,
                    } => {
                        retained = None;
                        let result = automation
                            .as_ref()
                            .ok_or_else(|| {
                                error("Не удалось открыть UI Automation для получения выделения.")
                            })
                            .and_then(|a| read(a, operation, &cancel, fallback, &mut retained));
                        if cancel.is_cancelled() {
                            retained = None;
                        }
                        let _ = reply.send(result);
                        // _lease outlives the real provider call, including abandoned waiters.
                    }
                }
            }
            drop(retained);
            drop(automation);
            if initialized {
                CoUninitialize();
            }
            drop(cleanup);
        })?;
    Ok(Worker { tx, closed })
});

// This check is invoked only by an explicit replacement, never ambient guards.
fn verify(operation: u64, original: CapturedSelection, cancel: OperationCancellation) -> bool {
    let Some(Ok(worker)) = Lazy::get(&WORKER) else {
        return false;
    };
    if worker.closed.load(Ordering::Acquire) {
        return false;
    }
    let Ok(lease) = crate::application::updates::activity::lease() else {
        return false;
    };
    let (reply, result) = bounded(1);
    if worker
        .tx
        .try_send(Command::Verify {
            operation,
            original,
            cancel,
            _lease: lease,
            reply,
        })
        .is_err()
    {
        return false;
    }
    result.recv().unwrap_or(false)
}

unsafe fn verify_range(
    a: &IUIAutomation,
    operation: u64,
    original: &CapturedSelection,
    retained: Option<&(u64, IUIAutomationTextRange)>,
) -> bool {
    let Some((owner, saved)) = retained.filter(|(id, _)| *id == operation) else {
        return false;
    };
    if *owner != operation || !original.belongs_to(operation) {
        return false;
    }
    let Some((target, element)) = super::super::target::selection_target(a) else {
        return false;
    };
    if target != original.target {
        return false;
    }
    let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    else {
        return false;
    };
    let Ok(ranges) = pattern.GetSelection() else {
        return false;
    };
    if ranges.Length().ok() != Some(1) {
        return false;
    }
    let Ok(current) = ranges.GetElement(0) else {
        return false;
    };
    if !current.Compare(saved).map(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    current
        .GetText((MAX_UTF16 + 1) as i32)
        .map(|text| text == original.text.as_str())
        .unwrap_or(false)
        && super::super::target::matches(&original.target)
}

pub(super) fn replace(
    operation: u64,
    original: CapturedSelection,
    text: &str,
    mode: crate::types::InjectionMode,
    cancel: OperationCancellation,
) -> AppResult<super::super::target::AppendResult> {
    // A clipboard fallback cannot perform a nested Ctrl+C validation while the
    // same clipboard owner prepares Ctrl+V. Refuse that combination, never skip proof.
    if original.source == SelectionSource::Clipboard
        && mode == crate::types::InjectionMode::Clipboard
    {
        return Err(error(
            "Это поле не поддерживает безопасную замену через буфер. Скопируйте результат.",
        ));
    }
    let target = original.target.clone();
    let validation_cancel = cancel.clone();
    let preflight = Arc::new(move || {
        if validation_cancel.is_cancelled() {
            return false;
        }
        match original.anchor {
            SelectionAnchor::UiAutomation { .. } => {
                verify(operation, original.clone(), validation_cancel.clone())
            }
            SelectionAnchor::ClassicEdit { start, end } => {
                matches_classic(&original.target, start, end)
                    && super::super::clipboard::selection_text(
                        &original.target,
                        start,
                        end,
                        validation_cancel.clone(),
                    )
                    .map(|text| text == original.text)
                    .unwrap_or(false)
                    && matches_classic(&original.target, start, end)
            }
        }
    });
    super::super::target::append_with_preflight(&target, text, mode, &cancel, Some(preflight))
}

pub(super) fn capture(
    operation: u64,
    cancel: OperationCancellation,
    fallback: bool,
    lease: ActivityLease<'static>,
) -> AppResult<CapturedSelection> {
    let worker = WORKER
        .as_ref()
        .map_err(|_| error("Не удалось открыть UI Automation для получения выделения."))?;
    if worker.closed.load(Ordering::Acquire) {
        return Err(error("Получение выделения недоступно."));
    }
    let (reply, result) = bounded(1);
    worker
        .tx
        .try_send(Command::Read {
            operation,
            cancel,
            fallback,
            _lease: lease,
            reply,
        })
        .map_err(|_| error("Предыдущее получение выделения ещё завершается."))?;
    result
        .recv()
        .map_err(|_| error("Не удалось получить выделение."))?
}
pub(super) fn release(operation: u64) {
    if let Some(Ok(worker)) = Lazy::get(&WORKER) {
        let _lease = crate::application::updates::activity::cleanup_lease();
        let _ = worker.tx.try_send(Command::Release { operation, _lease });
    }
}
pub(super) fn shutdown() {
    if let Some(Ok(worker)) = Lazy::get(&WORKER) {
        worker.closed.store(true, Ordering::Release);
        let _lease = crate::application::updates::activity::cleanup_lease();
        let _ = worker.tx.try_send(Command::Release {
            operation: 0,
            _lease,
        });
    }
}

unsafe fn read(
    a: &IUIAutomation,
    operation: u64,
    cancel: &OperationCancellation,
    fallback: bool,
    retained: &mut Option<(u64, IUIAutomationTextRange)>,
) -> AppResult<CapturedSelection> {
    if cancel.is_cancelled() {
        return Err(error("Получение выделения отменено."));
    }
    let (target, element) = super::super::target::selection_target(a)
        .ok_or_else(|| error("Выберите выделение в доступном текстовом поле другого приложения. Защищённые поля недоступны."))?;
    let pattern = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId);
    let pattern = match pattern {
        Ok(pattern) => pattern,
        Err(e) if e.code().0 as u32 == UIA_E_NOTSUPPORTED => {
            if !fallback {
                return Err(error("Поле не поддерживает получение выделения через UI Automation. Резервный Ctrl+C выключен."));
            }
            let (start, end) = classic_selection(&target)?;
            let text =
                super::super::clipboard::selection_text(&target, start, end, cancel.clone())?;
            if cancel.is_cancelled() {
                return Err(error("Получение выделения отменено."));
            }
            return captured(
                text,
                SelectionSource::Clipboard,
                target,
                SelectionAnchor::ClassicEdit { start, end },
            );
        }
        Err(_) => return Err(error("Не удалось получить выделение через UI Automation.")),
    };
    let ranges = pattern
        .GetSelection()
        .map_err(|_| error("Не удалось получить выделение через UI Automation."))?;
    if ranges.Length().ok() != Some(1) {
        return Err(error("Выберите один непрерывный фрагмент текста."));
    }
    let range = ranges
        .GetElement(0)
        .map_err(|_| error("Нет выделенного текста. Выделите фрагмент и повторите действие."))?;
    if range
        .CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &range,
            TextPatternRangeEndpoint_End,
        )
        .map_err(|_| error("Не удалось зафиксировать выделение."))?
        >= 0
    {
        return Err(error(
            "Нет выделенного текста. Выделите фрагмент и повторите действие.",
        ));
    }
    let text = range
        .GetText((MAX_UTF16 + 1) as i32)
        .map_err(|_| error("Не удалось прочитать выделенный фрагмент."))?
        .to_string();
    let text = validate_text(text)?;
    if cancel.is_cancelled() || !super::super::target::matches(&target) {
        return Err(error(
            "Поле или выделение изменилось. Выделите фрагмент заново.",
        ));
    }
    let current = pattern
        .GetSelection()
        .map_err(|_| error("Поле или выделение изменилось. Выделите фрагмент заново."))?;
    if current.Length().ok() != Some(1)
        || !range
            .Compare(
                &current.GetElement(0).map_err(|_| {
                    error("Поле или выделение изменилось. Выделите фрагмент заново.")
                })?,
            )
            .map(|v| v.as_bool())
            .unwrap_or(false)
    {
        return Err(error(
            "Поле или выделение изменилось. Выделите фрагмент заново.",
        ));
    }
    let anchor = range
        .Clone()
        .map_err(|_| error("Не удалось зафиксировать выделение."))?;
    *retained = Some((operation, anchor));
    captured(
        text,
        SelectionSource::UiAutomation,
        target,
        SelectionAnchor::UiAutomation { operation },
    )
}

pub(crate) fn classic_selection(target: &TextTarget) -> AppResult<(u32, u32)> {
    let window = HWND(target.focus as *mut _);
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(window, &mut class) };
    let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
    if !["EDIT", "RICHEDIT20W", "RICHEDIT50W", "RICHEDIT20A"]
        .iter()
        .any(|name| class.eq_ignore_ascii_case(name))
    {
        return Err(error(
            "Резервный Ctrl+C недоступен: приложение не подтверждает границы выделения.",
        ));
    }
    let mut start = 0u32;
    let mut end = 0u32;
    // EM_GETSEL is a system message below WM_USER; USER32 marshals its two DWORDs.
    let ok = unsafe {
        SendMessageTimeoutW(
            window,
            0x00b0,
            WPARAM((&mut start as *mut u32) as usize),
            LPARAM((&mut end as *mut u32) as isize),
            SMTO_ABORTIFHUNG,
            100,
            None,
        )
    };
    if ok.0 == 0 || start == end || start.max(end) - start.min(end) > MAX_UTF16 as u32 {
        return Err(error(
            "Нет подтверждённого непустого выделения для резервного Ctrl+C.",
        ));
    }
    Ok((start.min(end), start.max(end)))
}

pub(crate) fn matches_classic(target: &TextTarget, start: u32, end: u32) -> bool {
    super::super::target::matches(target) && classic_selection(target).ok() == Some((start, end))
}
