use crate::error::{AppError, AppResult};

/// Fixed application metadata. IPC does not accept arbitrary URLs or commands.
pub const PROJECT_SITE_URL: &str = "https://fono.gorbach-dev.ru/";

#[cfg(windows)]
pub fn open() -> AppResult<()> {
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::HWND,
            UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        },
    };
    let url: Vec<u16> = PROJECT_SITE_URL.encode_utf16().chain(Some(0)).collect();
    // The URL buffer lives through this synchronous call and is NUL-terminated.
    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("open"),
            PCWSTR(url.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        return Err(AppError::Config(
            "Не удалось открыть сайт Fono. Проверьте браузер по умолчанию в Windows.".into(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn open() -> AppResult<()> {
    Err(AppError::Config(
        "Открытие сайта доступно в Windows-сборке Fono.".into(),
    ))
}
