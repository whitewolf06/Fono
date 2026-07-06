//! Текст-инъекция в активное окно через Win32 `SendInput`.
//!
//! Использует Unicode KEYEVENTF_UNICODE для поддержки русского, эмодзи и CJK.
//! Для очень длинных текстов можно переключиться на режим "через буфер обмена"
//! (TODO: `inject_via_clipboard`).

use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Вставляет текст в окно, имеющее фокус клавиатуры.
pub fn inject_text(text: &str) -> AppResult<()> {
    if text.is_empty() {
        return Ok(());
    }
    tracing::debug!("injecting {} chars", text.chars().count());
    inject_text_sendinput(text)?;
    Ok(())
}

#[cfg(windows)]
fn inject_text_sendinput(text: &str) -> AppResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

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
fn build_unicode_input(u: u16) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE,
    };

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

/// Резервный способ для длинных текстов: положить в буфер обмена + Ctrl+V.
/// TODO (Этап 2): реализовать с восстановлением предыдущего буфера.
#[allow(dead_code)]
pub fn inject_via_clipboard(_text: &str) -> AppResult<()> {
    Err(AppError::Injection(
        "вставка через буфер обмена ещё не реализована".into(),
    ))
}
