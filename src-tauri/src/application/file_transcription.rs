//! Native file selection/export. Dialog objects stay on the UI owning thread;
//! decode/inference and filesystem writes run on background workers.
use super::transcript_export::ExportFormat;
use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use tauri::AppHandle;
#[cfg(windows)]
use tauri::Manager;

#[derive(Default, serde::Serialize)]
pub struct SubmitFilesResult {
    pub accepted: Vec<String>,
    pub rejected: usize,
}

pub async fn dialog(app: AppHandle, export: Option<ExportFormat>) -> AppResult<Vec<PathBuf>> {
    let activity = super::updates::activity::lease()?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let owner = app.clone();
    app.run_on_main_thread(move || {
        // Closing/cancelling the IPC waiter never releases the actual dialog lease.
        let _activity = activity;
        let _ = sender.send(native_dialog(&owner, export));
    })
    .map_err(|_| AppError::Internal("Не удалось открыть выбор файла.".into()))?;
    receiver
        .await
        .map_err(|_| AppError::Internal("Выбор файла не завершился.".into()))?
}

#[cfg(windows)]
fn native_dialog(app: &AppHandle, export: Option<ExportFormat>) -> AppResult<Vec<PathBuf>> {
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::UI::Controls::Dialogs::*,
    };
    let owner = app
        .get_webview_window("settings")
        .ok_or_else(|| AppError::Internal("Основное окно недоступно.".into()))?;
    let hwnd = owner
        .hwnd()
        .map_err(|_| AppError::Internal("Основное окно недоступно.".into()))?;
    let mut buffer = vec![0u16; 65_536];
    let filter = match export {
        None => "Аудиофайлы\0*.wav;*.mp3;*.flac;*.ogg\0\0".to_string(),
        Some(format) => format!(
            "{}\0*.{}\0\0",
            format.extension().to_uppercase(),
            format.extension()
        ),
    }
    .encode_utf16()
    .collect::<Vec<_>>();
    let extension = export
        .map(|f| {
            format!("{}\0", f.extension())
                .encode_utf16()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: windows::Win32::Foundation::HWND(hwnd.0),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrDefExt: if extension.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(extension.as_ptr())
        },
        Flags: OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if export.is_some() {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_ALLOWMULTISELECT | OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    let accepted = unsafe {
        if export.is_some() {
            GetSaveFileNameW(&mut options)
        } else {
            GetOpenFileNameW(&mut options)
        }
    }
    .as_bool();
    if !accepted {
        if unsafe { CommDlgExtendedError() }.0 != 0 {
            return Err(AppError::Internal(
                "Не удалось выбрать файл. Попробуйте выбрать меньше файлов.".into(),
            ));
        }
        return Ok(Vec::new());
    }
    Ok(selection_paths(&buffer))
}

#[cfg(not(windows))]
fn native_dialog(_: &AppHandle, _: Option<ExportFormat>) -> AppResult<Vec<PathBuf>> {
    Err(AppError::Config("Выбор файлов доступен в Windows.".into()))
}

#[cfg(any(windows, test))]
fn selection_paths(buffer: &[u16]) -> Vec<PathBuf> {
    let parts: Vec<_> = buffer
        .split(|c| *c == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    match parts.as_slice() {
        [] => Vec::new(),
        [single] => vec![PathBuf::from(single)],
        [directory, names @ ..] => names
            .iter()
            .map(|n| PathBuf::from(directory).join(n))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_selection_preserves_spaces_and_unicode() {
        let buffer: Vec<_> = "C:\\Моя музыка\0первая запись.wav\0вторая.flac\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(
            selection_paths(&buffer),
            vec![
                PathBuf::from("C:\\Моя музыка").join("первая запись.wav"),
                PathBuf::from("C:\\Моя музыка").join("вторая.flac")
            ]
        );
    }
}
