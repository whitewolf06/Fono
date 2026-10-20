//! WH_*_LL callbacks run only on this thread. Never query text, focus or STT here.
use super::{Edge, Queue};
use crate::types::DictationTrigger;
use std::{
    cell::RefCell,
    sync::{atomic::Ordering, mpsc, Arc},
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
};

thread_local! {
    static CALLBACK: RefCell<Option<Callback>> = const { RefCell::new(None) };
}

struct Callback {
    queue: Arc<Queue>,
    trigger: DictationTrigger,
    down: bool,
    primed: bool,
    suppressed: bool,
}

impl Callback {
    fn keyboard(&mut self, vk: u32, injected: bool, pressed: bool, time: u32) -> bool {
        if injected || vk != virtual_key(self.trigger) as u32 {
            return false;
        }
        self.edge(pressed, time)
    }

    fn mouse(&mut self, button: u32, injected: bool, pressed: bool, time: u32) -> bool {
        let expected = if self.trigger == DictationTrigger::XButton1 {
            1
        } else {
            2
        };
        if injected || button != expected {
            return false;
        }
        self.edge(pressed, time)
    }

    fn edge(&mut self, pressed: bool, time: u32) -> bool {
        // Pair a swallowed CapsLock/mouse down with its release even on overflow.
        let suppress = matches!(
            self.trigger,
            DictationTrigger::CapsLock | DictationTrigger::XButton1 | DictationTrigger::XButton2
        );
        if !self.queue.enabled.load(Ordering::Acquire) {
            let swallow = self.suppressed;
            if !pressed {
                self.suppressed = false;
            }
            return swallow;
        }
        if !self.primed {
            if !pressed {
                self.primed = true;
                self.down = false;
            }
            return false; // Installation while held cannot start a capture.
        }
        if suppress {
            self.suppressed = pressed;
        }
        if self.down != pressed {
            self.down = pressed;
            self.queue.push(Edge { pressed, time });
        }
        suppress
    }
}

fn virtual_key(trigger: DictationTrigger) -> i32 {
    match trigger {
        DictationTrigger::RightAlt => 0xA5,
        DictationTrigger::RightCtrl => 0xA3,
        DictationTrigger::CapsLock => 0x14,
        DictationTrigger::XButton1 => 0x05,
        DictationTrigger::XButton2 => 0x06,
        DictationTrigger::Shortcut => 0,
    }
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: Windows supplies this structure for the callback's duration.
        let input = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let pressed = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        let swallow = CALLBACK.with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return false;
            };
            let Some(callback) = slot.as_mut() else {
                return false;
            };
            // Ignore synthetic input (including Fono's own insertion); only the
            // chosen modifier is retained. No scan codes/characters enter Queue.
            pressed.is_some_and(|pressed| {
                callback.keyboard(
                    input.vkCode,
                    input.flags.contains(LLKHF_INJECTED),
                    pressed,
                    input.time,
                )
            })
        });
        if swallow {
            return LRESULT(1);
        }
    }
    // SAFETY: unhandled/negative-code events must continue through the chain.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && lparam.0 != 0
        && matches!(wparam.0 as u32, WM_XBUTTONDOWN | WM_XBUTTONUP)
    {
        // Coordinates/wheel/other buttons are neither copied nor queued.
        let input = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let swallow = CALLBACK.with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return false;
            };
            let Some(callback) = slot.as_mut() else {
                return false;
            };
            callback.mouse(
                input.mouseData >> 16,
                input.flags & LLMHF_INJECTED != 0,
                wparam.0 as u32 == WM_XBUTTONDOWN,
                input.time,
            )
        });
        if swallow {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

struct NativeResources {
    hook: HHOOK,
    timer: usize,
}

impl Drop for NativeResources {
    fn drop(&mut self) {
        // All native objects stay on their creating message-loop thread.
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
            let _ = KillTimer(None, self.timer);
        }
        CALLBACK.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(super) struct HookThread {
    queue: Arc<Queue>,
    id: u32,
    join: Option<JoinHandle<()>>,
}

impl HookThread {
    pub fn start(queue: Arc<Queue>, trigger: DictationTrigger) -> Result<Self, &'static str> {
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread_queue = queue.clone();
        let join = thread::Builder::new()
            .name("fono-input-hooks".into())
            .spawn(move || {
                let mut message = MSG::default();
                // Create the thread queue before publishing its id. The timer also
                // guarantees shutdown if PostThreadMessage fails or its queue fills.
                unsafe {
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                }
                let timer = unsafe { SetTimer(None, 0, 50, None) };
                if timer == 0 {
                    let _ = ready.send(Err("Не удалось запустить триггер."));
                    return;
                }
                let keyboard_trigger = trigger.is_keyboard();
                let module = match unsafe { GetModuleHandleW(None) } {
                    Ok(module) => HINSTANCE(module.0),
                    Err(_) => {
                        unsafe {
                            let _ = KillTimer(None, timer);
                        }
                        let _ = ready.send(Err("Не удалось подключить триггер."));
                        return;
                    }
                };
                CALLBACK.with(|slot| {
                    // GetAsyncKeyState is outside the callback, once at startup.
                    let held = unsafe { GetAsyncKeyState(virtual_key(trigger)) } < 0;
                    *slot.borrow_mut() = Some(Callback {
                        queue: thread_queue.clone(),
                        trigger,
                        down: held,
                        primed: !held,
                        suppressed: false,
                    });
                });
                let hook = unsafe {
                    SetWindowsHookExW(
                        if keyboard_trigger {
                            WH_KEYBOARD_LL
                        } else {
                            WH_MOUSE_LL
                        },
                        if keyboard_trigger {
                            Some(keyboard)
                        } else {
                            Some(mouse)
                        },
                        module,
                        0,
                    )
                };
                let Ok(hook) = hook else {
                    unsafe {
                        let _ = KillTimer(None, timer);
                    }
                    CALLBACK.with(|slot| {
                        slot.borrow_mut().take();
                    });
                    let _ = ready.send(Err(
                        "Не удалось подключить триггер. Выберите обычное сочетание клавиш.",
                    ));
                    return;
                };
                let _resources = NativeResources { hook, timer };
                let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
                while !thread_queue.stopping.load(Ordering::Acquire) {
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
                    if result <= 0 {
                        break;
                    }
                    // No window messages need dispatch: only LL callbacks, quit and timer.
                }
                if !thread_queue.stopping.load(Ordering::Acquire) {
                    thread_queue.fail();
                }
            })
            .map_err(|_| "Не удалось запустить триггер.")?;
        match receiver.recv() {
            Ok(Ok(id)) => Ok(Self {
                queue,
                id,
                join: Some(join),
            }),
            result => {
                let _ = join.join();
                Err(result
                    .ok()
                    .and_then(Result::err)
                    .unwrap_or("Не удалось запустить триггер."))
            }
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        self.queue.enabled.store(false, Ordering::Release);
        self.queue.stopping.store(true, Ordering::Release);
        unsafe {
            let _ = PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(test)]
mod tests;
