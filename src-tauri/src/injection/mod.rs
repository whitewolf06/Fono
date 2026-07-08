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
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    unsafe {
        // Проверяем, что есть окно с фокусом — иначе ввод уйдёт в никуда.
        if GetForegroundWindow().0.is_null() {
            return Err(AppError::Injection(
                "нет активного окна для ввода".into(),
            ));
        }
    }

    const BATCH: usize = 16;
    let chars: Vec<char> = text.chars().collect();
    let mut total_sent = 0usize;

    for chunk in chars.chunks(BATCH) {
        let inputs: Vec<INPUT> = chunk
            .iter()
            .flat_map(|&c| {
                let s: String = c.to_string();
                s.encode_utf16()
                    .map(build_unicode_input)
                    .collect::<Vec<_>>()
            })
            .collect();

        unsafe {
            let cbsize = std::mem::size_of::<INPUT>() as i32;
            let sent = SendInput(&inputs, cbsize);
            if sent == 0 {
                return Err(AppError::Injection(
                    "SendInput вернул 0 (ошибка ввода)".into(),
                ));
            }
            total_sent += sent as usize;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    tracing::info!("injected {} input events", total_sent);
    Ok(())
}

#[cfg(windows)]
fn build_unicode_input(u: u16) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: u,
                dwExtraInfo: 0,
                time: 0,
                dwFlags: KEYEVENTF_UNICODE,
            },
        },
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
    unsafe {
        if GetForegroundWindow().0.is_null() {
            return Err(AppError::Injection(
                "нет активного окна для ввода".into(),
            ));
        }
    }

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| AppError::Injection(format!("буфер обмена: {e}")))?;

    // Сохраняем старое текстовое содержимое, если оно было.
    let old_text = clipboard.get_text().ok();

    // Кладём наш текст.
    clipboard
        .set_text(text.to_string())
        .map_err(|e| AppError::Injection(format!("буфер обмена: {e}")))?;

    // Эмулируем Ctrl+V.
    send_ctrl_v()?;

    // Восстанавливаем старый буфер с небольшой задержкой, чтобы приложение успело вставить.
    let old_text = old_text.unwrap_or_default();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(old_text);
        }
    });

    tracing::info!("injected {} chars via clipboard", text.chars().count());
    Ok(())
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

    unsafe {
        let cbsize = std::mem::size_of::<INPUT>() as i32;
        let sent = SendInput(&inputs, cbsize);
        if sent == 0 {
            return Err(AppError::Injection(
                "SendInput(Ctrl+V) вернул 0".into(),
            ));
        }
    }

    // Даём приложению время обработать вставку.
    std::thread::sleep(Duration::from_millis(150));
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
