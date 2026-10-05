//! Runs the already verified package in its normal interactive NSIS wizard.
//! Download cancellation never terminates an installer or an installed Fono.

mod installed;
mod msi;

use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0},
        System::{
            Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
            Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
        },
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Cancelled,
}

struct ProcessHandle(HANDLE);

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // We own the handle only. Closing it never terminates the installer.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn launch_verified_package(path: &Path, version: &str) -> Result<Outcome, String> {
    installed::require_compatible(version)?;
    let path = path.canonicalize().map_err(|error| {
        format!("Не удалось открыть проверенный установщик: {error}. Нажмите «Повторить».")
    })?;
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let directory: Vec<u16> = path
        .parent()
        .unwrap_or(&path)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpDirectory: PCWSTR(directory.as_ptr()),
        // No silent flags: the user sees and controls the ordinary NSIS wizard.
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .map_err(|error| {
                format!("Не удалось подготовить запуск установщика Windows: {error}.")
            })?;
        let _apartment = ComApartment;
        if let Err(error) = ShellExecuteExW(&mut execution) {
            if error.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(Outcome::Cancelled);
            }
            return Err(format!(
                "Не удалось запустить установщик: {error}. Нажмите «Повторить»."
            ));
        }
        if execution.hProcess.is_invalid() {
            return Err("Windows не вернула процесс установщика. Откройте загруженный пакет вручную или нажмите «Повторить».".into());
        }
        let process = ProcessHandle(execution.hProcess);
        if WaitForSingleObject(process.0, INFINITE) != WAIT_OBJECT_0 {
            return Err("Не удалось получить результат установки. Проверьте окно установщика; загруженный файл сохранён.".into());
        }
        let mut code = 0;
        GetExitCodeProcess(process.0, &mut code)
            .map_err(|error| format!("Не удалось узнать результат установки: {error}."))?;
        classify_exit_code(code)
    }
}

fn classify_exit_code(code: u32) -> Result<Outcome, String> {
    match code {
        0 => Ok(Outcome::Completed),
        1 => Ok(Outcome::Cancelled),
        _ => Err(format!("Установка не завершилась (код {code}). Загруженный пакет сохранён; можно повторить попытку.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nsis_cancellation_is_not_reported_as_success() {
        assert_eq!(classify_exit_code(0).unwrap(), Outcome::Completed);
        assert_eq!(classify_exit_code(1).unwrap(), Outcome::Cancelled);
        assert!(classify_exit_code(2).is_err());
        assert!(classify_exit_code(1223).is_err());
    }
}
