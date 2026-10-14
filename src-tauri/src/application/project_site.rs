use crate::error::{AppError, AppResult};

/// Fixed application metadata. IPC does not accept arbitrary URLs or commands.
pub const PROJECT_SITE_URL: &str = "https://fono.gorbach-dev.ru/";

pub fn open() -> AppResult<()> {
    open_url(PROJECT_SITE_URL)
}

#[cfg(windows)]
pub(crate) fn open_url(target: &str) -> AppResult<()> {
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::HWND,
            UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        },
    };
    let url: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    // Callers supply only validated application metadata; IPC accepts no URLs.
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
            "Не удалось открыть браузер. Проверьте браузер по умолчанию в Windows.".into(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn open_url(_target: &str) -> AppResult<()> {
    Err(AppError::Config(
        "Открытие браузера доступно в Windows-сборке Fono.".into(),
    ))
}
