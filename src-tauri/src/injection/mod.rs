//! Текст-инъекция в активное окно.
//!
//! Два режима:
//! - `SendInput` — имитирует нажатия Unicode-клавиш (быстро, не трогает буфер обмена).
//! - `Clipboard` — кладёт текст в буфер обмена и эмулирует Ctrl+V (работает в Telegram и других
//!   приложениях, которые игнорируют synthetic input).

use std::time::Duration;

#[cfg(windows)]
use crossbeam_channel::{bounded, Sender};
#[cfg(windows)]
use once_cell::sync::Lazy;
#[cfg(windows)]
use parking_lot::Mutex;

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
pub fn copy_text(text: &str) -> AppResult<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::Injection(format!("clipboard: {e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| AppError::Injection(format!("clipboard: {e}")))
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
    CLIPBOARD_WORKER.inject(text.to_owned())
}

#[cfg(windows)]
enum ClipboardCommand {
    Inject {
        text: String,
        response: Sender<AppResult<()>>,
    },
    Shutdown {
        response: Sender<()>,
    },
}

#[cfg(windows)]
struct ClipboardInjectionWorker {
    commands: Sender<ClipboardCommand>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[cfg(windows)]
impl ClipboardInjectionWorker {
    fn new() -> Self {
        let (commands, receiver) = bounded(4);
        let thread = std::thread::spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    ClipboardCommand::Inject { text, response } => {
                        let _ = response.send(inject_clipboard_on_owner_thread(&text));
                    }
                    ClipboardCommand::Shutdown { response } => {
                        let _ = response.send(());
                        break;
                    }
                }
            }
        });
        Self {
            commands,
            thread: Mutex::new(Some(thread)),
        }
    }

    fn inject(&self, text: String) -> AppResult<()> {
        let (response_tx, response_rx) = bounded(1);
        self.commands
            .send(ClipboardCommand::Inject {
                text,
                response: response_tx,
            })
            .map_err(|_| AppError::Injection("clipboard injection worker stopped".into()))?;
        response_rx
            .recv()
            .map_err(|_| AppError::Injection("clipboard injection worker disconnected".into()))?
    }

    fn shutdown(&self) {
        let (response_tx, response_rx) = bounded(1);
        let _ = self.commands.send(ClipboardCommand::Shutdown {
            response: response_tx,
        });
        let _ = response_rx.recv_timeout(Duration::from_secs(1));
        if let Some(thread) = self.thread.lock().take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
static CLIPBOARD_WORKER: Lazy<ClipboardInjectionWorker> = Lazy::new(ClipboardInjectionWorker::new);

#[cfg(windows)]
fn inject_clipboard_on_owner_thread(text: &str) -> AppResult<()> {
    let target = unsafe { GetForegroundWindow() };
    if target.0.is_null() {
        return Err(AppError::Injection("нет активного окна для ввода".into()));
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::Injection(format!("буфер обмена: {e}")))?;

    let previous = if let Ok(text) = clipboard.get_text() {
        ClipboardBackup::Text(text)
    } else if let Ok(files) = clipboard.get().file_list() {
        ClipboardBackup::Files(files)
    } else if let Ok(image) = clipboard.get_image() {
        ClipboardBackup::Image(image)
    } else {
        ClipboardBackup::Empty
    };

    // Кладём наш текст.
    clipboard
        .set_text(text.to_string())
        .map_err(|e| AppError::Injection(format!("буфер обмена: {e}")))?;

    // Эмулируем Ctrl+V.
    if unsafe { GetForegroundWindow() } != target {
        restore_clipboard(&mut clipboard, previous)?;
        return Err(AppError::Injection(
            "активное окно изменилось перед вставкой".into(),
        ));
    }
    send_ctrl_v()?;

    // Restore on the same bounded owner thread. Do not overwrite something the
    // user copied while the target application was consuming Ctrl+V.
    std::thread::sleep(Duration::from_millis(300));
    if clipboard.get_text().ok().as_deref() == Some(text) {
        restore_clipboard(&mut clipboard, previous)?;
    }

    tracing::info!("injected {} chars via clipboard", text.chars().count());
    Ok(())
}

#[cfg(windows)]
enum ClipboardBackup {
    Text(String),
    Files(Vec<std::path::PathBuf>),
    Image(arboard::ImageData<'static>),
    Empty,
}

#[cfg(windows)]
fn restore_clipboard(
    clipboard: &mut arboard::Clipboard,
    previous: ClipboardBackup,
) -> AppResult<()> {
    let result = match previous {
        ClipboardBackup::Text(text) => clipboard.set_text(text),
        ClipboardBackup::Files(files) => clipboard.set().file_list(&files),
        ClipboardBackup::Image(image) => clipboard.set_image(image),
        ClipboardBackup::Empty => clipboard.clear(),
    };
    result.map_err(|error| AppError::Injection(format!("restore clipboard: {error}")))
}

pub fn shutdown() {
    #[cfg(windows)]
    if let Some(worker) = Lazy::get(&CLIPBOARD_WORKER) {
        worker.shutdown();
    }
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
        if sent != inputs.len() as u32 {
            // A partial sequence can leave Ctrl logically pressed. Always try to
            // release both keys before surfacing the failure.
            let releases = [up_v, up_ctrl];
            let _ = SendInput(&releases, cbsize);
            return Err(AppError::Injection(format!(
                "SendInput(Ctrl+V) отправил только {sent} из {} событий",
                inputs.len()
            )));
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
