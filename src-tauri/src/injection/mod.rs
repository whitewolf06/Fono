//! Текст-инъекция в активное окно.
//!
//! Два режима:
//! - `SendInput` — имитирует нажатия Unicode-клавиш (быстро, не трогает буфер обмена).
//! - `Clipboard` — кладёт текст в буфер обмена и эмулирует Ctrl+V (работает в Telegram и других
//!   приложениях, которые игнорируют synthetic input).

use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::types::InjectionMode;
#[cfg(windows)]
mod clipboard;
mod modifiers;
pub mod target;

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Вставляет текст в окно, имеющее фокус клавиатуры, выбранным способом.
pub fn inject_text(text: &str, mode: InjectionMode) -> AppResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    tracing::debug!("injecting {} chars via {:?}", text.chars().count(), mode);
    match mode {
        InjectionMode::SendInput => inject_text_sendinput(text),
        InjectionMode::Clipboard => inject_via_clipboard(text),
    }
}

#[cfg(windows)]
pub fn copy_text(text: &str) -> AppResult<()> {
    clipboard::copy(text)
}

#[cfg(not(windows))]
pub fn copy_text(_text: &str) -> AppResult<()> {
    Err(AppError::Config(
        "clipboard is supported only on Windows".into(),
    ))
}

#[cfg(windows)]
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    unsafe {
        // Проверяем, что есть окно с фокусом — иначе ввод уйдёт в никуда.
        if GetForegroundWindow().0.is_null() {
            return Err(AppError::Injection("нет активного окна для ввода".into()));
        }
    }

    const BATCH: usize = 16;
    let chars: Vec<char> = text.chars().collect();
    let mut total_sent = 0usize;

    for chunk in chars.chunks(BATCH) {
        modifiers::wait_for_release(|| false)?;
        let inputs: Vec<INPUT> = chunk
            .iter()
            .flat_map(|&c| {
                let s: String = c.to_string();
                s.encode_utf16()
                    .flat_map(build_unicode_inputs)
                    .collect::<Vec<_>>()
            })
            .collect();

        unsafe {
            let cbsize = std::mem::size_of::<INPUT>() as i32;
            let sent = SendInput(&inputs, cbsize);
            if sent != inputs.len() as u32 {
                release_unicode_units(chunk);
                return Err(AppError::Injection(format!(
                    "SendInput отправил только {sent} из {} событий Unicode",
                    inputs.len()
                )));
            }
            total_sent += sent as usize;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    tracing::info!("injected {} input events", total_sent);
    Ok(())
}

#[cfg(windows)]
fn build_unicode_inputs(u: u16) -> [INPUT; 2] {
    [build_unicode_input(u, false), build_unicode_input(u, true)]
}

#[cfg(windows)]
fn build_unicode_input(u: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: u,
                dwExtraInfo: 0,
                time: 0,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
            },
        },
    }
}

#[cfg(windows)]
fn release_unicode_units(chunk: &[char]) {
    let inputs: Vec<INPUT> = chunk
        .iter()
        .flat_map(|c| {
            c.to_string()
                .encode_utf16()
                .collect::<Vec<_>>()
                .into_iter()
                .map(|u| build_unicode_input(u, true))
        })
        .collect();
    if !inputs.is_empty() {
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

#[cfg(not(windows))]
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    let _ = text;
    Err(AppError::Injection(
        "текст-инъекция поддерживается только на Windows".into(),
    ))
}

/// Резервный способ для длинных текстов / проблемных приложений (Telegram и др.).
/// Кладёт текст в буфер обмена, эмулирует Ctrl+V, затем восстанавливает предыдущее содержимое.
#[cfg(windows)]
pub fn inject_via_clipboard(text: &str) -> AppResult<()> {
    clipboard::inject(text, None)
}

#[cfg(windows)]
pub fn inject_via_clipboard_checked(
    text: &str,
    target: target::TextTarget,
    cancel: fono_core::OperationCancellation,
) -> AppResult<()> {
    clipboard::inject(text, Some((target, cancel)))
}
#[cfg(not(windows))]
pub fn inject_via_clipboard_checked(
    _: &str,
    _: target::TextTarget,
    _: fono_core::OperationCancellation,
) -> AppResult<()> {
    Err(AppError::Injection(
        "Вставка поддерживается только в Windows".into(),
    ))
}

pub fn shutdown() {
    #[cfg(windows)]
    clipboard::shutdown();
}

#[cfg(windows)]
fn send_ctrl_v() -> AppResult<()> {
    const VK_CONTROL: u16 = 0x11;
    const VK_V: u16 = 0x56;

    let down_ctrl = key_input(VK_CONTROL, 0);
    let down_v = key_input(VK_V, 0);
    let up_v = key_input(VK_V, KEYEVENTF_KEYUP.0);
    let up_ctrl = key_input(VK_CONTROL, KEYEVENTF_KEYUP.0);

    let inputs = [down_ctrl, down_v, up_v, up_ctrl];

    modifiers::ensure_released()?;
    // Never emit our V-up while the user is physically holding V either.
    if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_V as i32) } as u16
        & 0x8000
        != 0
    {
        return Err(AppError::Injection("paste key is still held".into()));
    }

    unsafe {
        let cbsize = std::mem::size_of::<INPUT>() as i32;
        let sent = SendInput(&inputs, cbsize);
        if sent != inputs.len() as u32 {
            // Recover only synthetic downs accepted from our own prefix. The
            // modifier gate never emits key-up for keys held by the user.
            let releases: Vec<_> = match sent {
                1 | 3 => vec![up_ctrl],
                2 => vec![up_v, up_ctrl],
                _ => Vec::new(),
            };
            let _ = SendInput(&releases, cbsize);
            return Err(AppError::Injection(format!(
                "SendInput(Ctrl+V) отправил только {sent} из {} событий",
                inputs.len()
            )));
        }
    }

    Ok(())
}

#[cfg(windows)]
fn key_input(vk: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwExtraInfo: 0,
                time: 0,
                dwFlags: KEYBD_EVENT_FLAGS(flags),
            },
        },
    }
}

#[cfg(not(windows))]
pub fn inject_via_clipboard(_text: &str) -> AppResult<()> {
    Err(AppError::Injection(
        "вставка через буфер обмена поддерживается только на Windows".into(),
    ))
}
