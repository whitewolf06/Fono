//! Глобальное состояние приложения.
//!
//! Хранится в `tauri::State` и доступно из всех команд и модулей.
//! Содержит текущие настройки, состояние конвейера и закэшированные
//! ресурсы (загруженная whisper-модель, активный аудио-поток).

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::error::AppResult;
use crate::types::DictationHistoryEntry;
use crate::types::Settings;

static PERSISTENCE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pipeline_state: Mutex<crate::types::PipelineState>,
    pub dictation_paused: Mutex<bool>,
    pending_voice_command: Mutex<Option<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let state = Self {
            settings: Mutex::new(Settings::default()),
            pipeline_state: Mutex::new(crate::types::PipelineState::Idle),
            dictation_paused: Mutex::new(false),
            pending_voice_command: Mutex::new(None),
        };
        // Пробуем подгрузить сохранённые настройки с диска
        match load_settings() {
            Ok(Some(settings)) => *state.settings.lock() = settings,
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "could not load saved settings; using defaults"),
        }
        state
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        *self.settings.lock() = settings;
    }

    pub fn pipeline_state(&self) -> crate::types::PipelineState {
        *self.pipeline_state.lock()
    }

    pub fn set_pipeline_state(&self, state: crate::types::PipelineState) {
        *self.pipeline_state.lock() = state;
    }

    pub fn is_dictation_paused(&self) -> bool {
        *self.dictation_paused.lock()
    }

    pub fn set_dictation_paused(&self, paused: bool) {
        *self.dictation_paused.lock() = paused;
    }

    pub fn toggle_dictation_paused(&self) -> bool {
        let mut paused = self.dictation_paused.lock();
        *paused = !*paused;
        *paused
    }

    pub fn pending_voice_command(&self) -> Option<String> {
        self.pending_voice_command.lock().clone()
    }

    pub fn set_pending_voice_command(&self, command: Option<String>) {
        *self.pending_voice_command.lock() = command;
    }

    pub fn take_pending_voice_command(&self) -> Option<String> {
        self.pending_voice_command.lock().take()
    }
}

/// Каталог данных приложения (для настроек, моделей и т.д.).
pub fn app_data_dir() -> AppResult<std::path::PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| crate::error::AppError::Config("не найден data_dir".into()))?
        .join("Fono");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Путь к файлу настроек.
pub fn settings_path() -> AppResult<std::path::PathBuf> {
    Ok(app_data_dir()?.join("settings.json"))
}

pub fn history_path() -> AppResult<std::path::PathBuf> {
    Ok(app_data_dir()?.join("dictation-history.json"))
}

pub fn load_dictation_history() -> AppResult<Vec<DictationHistoryEntry>> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = history_path()?;
    Ok(load_json_with_backup(&path)?.unwrap_or_default())
}

pub fn append_dictation_history(entry: DictationHistoryEntry) -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = history_path()?;
    let mut entries: Vec<DictationHistoryEntry> = load_json_with_backup(&path)?.unwrap_or_default();
    entries.insert(0, entry);
    entries.truncate(200);
    save_json_atomically(&path, &entries)
}

pub fn clear_dictation_history() -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    save_json_atomically(&history_path()?, &Vec::<DictationHistoryEntry>::new())
}

pub fn delete_dictation_history_entry(id: &str) -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = history_path()?;
    let mut entries: Vec<DictationHistoryEntry> = load_json_with_backup(&path)?.unwrap_or_default();
    entries.retain(|entry| entry.id != id);
    save_json_atomically(&path, &entries)
}

/// Каталог для whisper-моделей.
pub fn models_dir() -> AppResult<std::path::PathBuf> {
    let dir = app_data_dir()?.join("whisper-models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn load_settings() -> AppResult<Option<Settings>> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = settings_path()?;
    let settings = load_json_with_backup(&path)?;
    if settings.is_some() {
        tracing::info!(?path, "loaded settings");
    }
    Ok(settings)
}

pub fn save_settings(settings: &Settings) -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = settings_path()?;
    save_json_atomically(&path, settings)?;
    tracing::info!(?path, "saved settings");
    Ok(())
}

fn load_json_with_backup<T>(path: &Path) -> AppResult<Option<T>>
where
    T: DeserializeOwned + Serialize,
{
    match read_json(path) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => {
            let backup = backup_path(path);
            match read_json(&backup) {
                Ok(Some(value)) => {
                    tracing::warn!(?path, ?backup, "primary JSON is missing; restoring backup");
                    if let Err(error) = restore_json_from_backup(path, &value) {
                        tracing::warn!(?path, %error, "could not restore JSON from backup");
                    }
                    Ok(Some(value))
                }
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            }
        }
        Err(primary_error) => {
            let backup = backup_path(path);
            match read_json(&backup) {
                Ok(Some(value)) => {
                    tracing::warn!(
                        ?path,
                        ?backup,
                        %primary_error,
                        "primary JSON is unavailable; restoring the last valid backup"
                    );
                    if let Err(error) = restore_json_from_backup(path, &value) {
                        tracing::warn!(?path, %error, "could not restore JSON from backup");
                    }
                    Ok(Some(value))
                }
                Ok(None) => Err(primary_error),
                Err(backup_error) => {
                    tracing::warn!(?backup, %backup_error, "JSON backup is also unavailable");
                    Err(primary_error)
                }
            }
        }
    }
}

fn read_json<T>(path: &Path) -> AppResult<Option<T>>
where
    T: DeserializeOwned,
{
    match fs::read_to_string(path) {
        Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn save_json_atomically<T>(path: &Path, value: &T) -> AppResult<()>
where
    T: Serialize,
{
    let raw = serde_json::to_vec_pretty(value)?;
    write_atomically(path, &raw, true)
}

fn restore_json_from_backup<T>(path: &Path, value: &T) -> AppResult<()>
where
    T: Serialize,
{
    let raw = serde_json::to_vec_pretty(value)?;
    write_atomically(path, &raw, false)
}

fn write_atomically(path: &Path, contents: &[u8], replace_backup: bool) -> AppResult<()> {
    let parent = path.parent().ok_or_else(|| {
        crate::error::AppError::Config(format!("persistence path has no parent: {path:?}"))
    })?;
    fs::create_dir_all(parent)?;

    let temporary = temporary_path(path);
    let write_result = (|| -> AppResult<()> {
        let mut file = File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);

        if path.exists() {
            let backup = replace_backup.then(|| backup_path(path));
            replace_existing_file(path, &temporary, backup.as_deref())?;
        } else {
            fs::rename(&temporary, path)?;
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn backup_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    path.with_extension(format!("{extension}.bak"))
}

fn temporary_path(path: &Path) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("data");
    path.with_file_name(format!(
        ".{filename}.{}.{}.tmp",
        std::process::id(),
        sequence
    ))
}

#[cfg(windows)]
fn replace_existing_file(path: &Path, temporary: &Path, backup: Option<&Path>) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH},
    };

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary_wide: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let backup_wide: Option<Vec<u16>> =
        backup.map(|backup| backup.as_os_str().encode_wide().chain(Some(0)).collect());

    if let Some(backup) = backup.filter(|backup| backup.exists()) {
        fs::remove_file(backup)?;
    }

    // ReplaceFileW replaces the destination atomically and commits the previous
    // complete file as the recovery backup. The temporary file is already synced.
    unsafe {
        ReplaceFileW(
            PCWSTR(path_wide.as_ptr()),
            PCWSTR(temporary_wide.as_ptr()),
            backup_wide
                .as_ref()
                .map_or(PCWSTR::null(), |backup| PCWSTR(backup.as_ptr())),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
        .map_err(|error| crate::error::AppError::Io(std::io::Error::other(error)))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_existing_file(path: &Path, temporary: &Path, backup: Option<&Path>) -> AppResult<()> {
    if let Some(backup) = backup {
        if backup.exists() {
            fs::remove_file(backup)?;
        }
        fs::copy(path, backup)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

/// Хранилище разделяемых аудио-чанков.
/// Продюсер = аудио-поток cpal, консьюмер = pipeline / wakeword / vad.
pub type AudioBuffer = Arc<Mutex<Vec<i16>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct StoredValue {
        value: String,
    }

    fn temporary_test_path(name: &str) -> PathBuf {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("fono-state-test-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&directory).expect("create isolated test directory");
        directory.join(name)
    }

    #[test]
    fn atomic_write_keeps_previous_complete_version_as_backup() {
        let path = temporary_test_path("settings.json");
        let first = StoredValue {
            value: "first".into(),
        };
        let second = StoredValue {
            value: "second".into(),
        };

        save_json_atomically(&path, &first).expect("write first version");
        save_json_atomically(&path, &second).expect("replace with second version");

        let current: StoredValue =
            serde_json::from_str(&fs::read_to_string(&path).expect("read current version"))
                .expect("deserialize current version");
        let backup: StoredValue = serde_json::from_str(
            &fs::read_to_string(backup_path(&path)).expect("read backup version"),
        )
        .expect("deserialize backup version");
        assert_eq!(current, second);
        assert_eq!(backup, first);

        fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
    }

    #[test]
    fn corrupted_primary_json_recovers_last_valid_backup() {
        let path = temporary_test_path("history.json");
        let previous = StoredValue {
            value: "previous".into(),
        };
        let current = StoredValue {
            value: "current".into(),
        };

        save_json_atomically(&path, &previous).expect("write previous version");
        save_json_atomically(&path, &current).expect("write current version");
        fs::write(&path, "{ invalid json").expect("corrupt primary JSON");

        let recovered = load_json_with_backup::<StoredValue>(&path)
            .expect("recover backup")
            .expect("backup value exists");
        assert_eq!(recovered, previous);

        let restored: StoredValue =
            serde_json::from_str(&fs::read_to_string(&path).expect("read restored primary"))
                .expect("deserialize restored primary");
        assert_eq!(restored, previous);

        let preserved_backup: StoredValue = serde_json::from_str(
            &fs::read_to_string(backup_path(&path)).expect("read preserved backup"),
        )
        .expect("deserialize preserved backup");
        assert_eq!(preserved_backup, previous);

        fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
    }
}
