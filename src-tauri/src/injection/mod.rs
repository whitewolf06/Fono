//! Keyboard-based text injection for the active window.
//!
//! Primary path uses SendInput + KEYEVENTF_UNICODE.
//! Fallback uses clipboard: copy text, Ctrl+V, and restore previous clipboard text.

use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Inject text as keyboard input into the active window.
pub fn inject_text(text: &str) -> AppResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    tracing::debug!("injecting {} chars", text.chars().count());

    if let Err(err) = inject_text_sendinput(text) {
        tracing::warn!("sendinput failed ({err}); fallback to clipboard injection");
        inject_via_clipboard(text)?;
    }

    Ok(())
}

#[cfg(windows)]
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        if GetForegroundWindow().0.is_null() {
            return Err(AppError::Injection(
                "No active foreground window. Text injection cannot continue.".into(),
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
                c.encode_utf16()
                    .flat_map(|u| [build_unicode_input(u, false), build_unicode_input(u, true)])
            })
            .collect();

        unsafe {
            let cbsize = std::mem::size_of::<INPUT>() as i32;
            let sent = SendInput(&inputs, cbsize);
            if sent == 0 {
                return Err(AppError::Injection(
                    "SendInput returned 0 (input system error).".into(),
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
fn build_unicode_input(
    u: u16,
    is_key_up: bool,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE,
    };

    let mut flags = KEYEVENTF_UNICODE;
    if is_key_up {
        flags |= windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: u,
                dwExtraInfo: 0,
                time: 0,
                dwFlags: flags,
            },
        },
    }
}

#[cfg(not(windows))]
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    let _ = text;
    Err(AppError::Injection(
        "Text injection is only supported on Windows.".into(),
    ))
}

/// Clipboard-based fallback for text injection with clipboard restore.
#[cfg(windows)]
pub fn inject_via_clipboard(text: &str) -> AppResult<()> {
    use std::thread::sleep;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS, VK_CONTROL, VK_V,
    };

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| AppError::Injection(format!("clipboard init failed: {e}")))?;

    let previous = clipboard.get_text().ok();
    clipboard
        .set_text(text)
        .map_err(|e| AppError::Injection(format!("clipboard set failed: {e}")))?;

    unsafe {
        keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        sleep(Duration::from_millis(20));
        keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }

    if let Some(previous_text) = previous {
        if let Err(e) = clipboard.set_text(&previous_text) {
            tracing::warn!("failed to restore clipboard: {e}");
        }
    } else if let Err(e) = clipboard.set_text("") {
        tracing::warn!("failed to clear clipboard: {e}");
    }

    Ok(())
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn inject_via_clipboard(_text: &str) -> AppResult<()> {
    Err(AppError::Injection(
        "Clipboard injection is only supported on Windows.".into(),
    ))
}
