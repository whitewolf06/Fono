//! Current-user startup registration. Never elevates or changes another app's entry.
use crate::error::{AppError, AppResult};

pub fn set_enabled(enabled: bool) -> AppResult<()> {
    if cfg!(debug_assertions) && std::env::var_os("FONO_TEST_DATA_DIR").is_some() {
        return Err(AppError::Config(
            "Автозапуск недоступен в изолированном тестовом профиле".into(),
        ));
    }
    set_enabled_platform(enabled)
}

#[cfg(windows)]
fn set_enabled_platform(enabled: bool) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut command = std::process::Command::new("reg.exe");
    command.creation_flags(0x0800_0000);
    if enabled {
        let exe = std::env::current_exe()?;
        command.args(["add", key, "/v", "Fono", "/t", "REG_SZ", "/d"]);
        command.arg(format!("\"{}\"", exe.display())).arg("/f");
    } else {
        // Query this exact value first: an absent entry is already disabled.
        let result = std::process::Command::new("reg.exe")
            .creation_flags(0x0800_0000)
            .args(["query", key, "/v", "Fono"])
            .output()?;
        if !result.status.success() {
            return Ok(());
        }
        command.args(["delete", key, "/v", "Fono", "/f"]);
    }
    if !command.output()?.status.success() {
        return Err(AppError::Config(
            "Windows не разрешила изменить автозапуск Fono".into(),
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn set_enabled_platform(_enabled: bool) -> AppResult<()> {
    Err(AppError::Config(
        "Автозапуск поддерживается только в Windows".into(),
    ))
}
