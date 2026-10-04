//! Глобальное состояние приложения.
//!
//! Хранится в `tauri::State` и доступно из всех команд и модулей.
//! Содержит текущие настройки, состояние конвейера и закэшированные
//! ресурсы (загруженная whisper-модель, активный аудио-поток).

mod settings_delta;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::error::{AppError, AppResult};
use crate::types::{CommandProposal, CommandSettingsSnapshot, Settings};

static PERSISTENCE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PERSISTENCE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize, Serialize)]
struct VersionedDocument<T> {
    schema_version: u16,
    data: T,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pipeline_state: Mutex<crate::types::PipelineState>,
    pub dictation_paused: Mutex<bool>,
    settings_version: AtomicU64,
    next_command_proposal_id: AtomicU64,
    pending_command_proposal: Mutex<Option<CommandProposal>>,
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
            settings_version: AtomicU64::new(1),
            next_command_proposal_id: AtomicU64::new(1),
            pending_command_proposal: Mutex::new(None),
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

    pub fn set_settings(&self, mut settings: Settings) {
        settings.enforce_classic_dictation();
        settings.enforce_wake_availability();
        settings.migrate_processing_prompts();
        *self.settings.lock() = settings;
        self.settings_version.fetch_add(1, Ordering::SeqCst);
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

    pub fn command_settings_snapshot(&self) -> CommandSettingsSnapshot {
        let settings = self.settings();
        CommandSettingsSnapshot {
            version: self.settings_version.load(Ordering::SeqCst),
            launch_apps: settings.launch_apps,
            volume_step: settings.volume_step,
        }
    }

    pub fn next_command_proposal_id(&self) -> u64 {
        self.next_command_proposal_id
            .fetch_add(1, Ordering::Relaxed)
    }

    pub fn pending_command_proposal(&self) -> Option<CommandProposal> {
        let mut pending = self.pending_command_proposal.lock();
        if pending
            .as_ref()
            .is_some_and(|proposal| proposal.is_expired_at(chrono::Utc::now()))
        {
            *pending = None;
        }
        pending.clone()
    }

    pub fn set_pending_command_proposal(&self, proposal: Option<CommandProposal>) {
        *self.pending_command_proposal.lock() = proposal;
    }

    pub fn take_pending_command_proposal(&self) -> AppResult<CommandProposal> {
        let proposal = self
            .pending_command_proposal
            .lock()
            .take()
            .ok_or_else(|| AppError::Config("No pending voice command".into()))?;
        if proposal.is_expired_at(chrono::Utc::now()) {
            return Err(AppError::Config("Voice command proposal expired".into()));
        }
        let current_version = self.settings_version.load(Ordering::SeqCst);
        if proposal.settings_version != current_version {
            return Err(AppError::Config(
                "Voice command proposal was invalidated by settings changes".into(),
            ));
        }
        Ok(proposal)
    }
}

/// Каталог данных приложения (для настроек, моделей и т.д.).
pub fn app_data_dir() -> AppResult<std::path::PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(path) = std::env::var_os("FONO_TEST_DATA_DIR") {
            let dir = PathBuf::from(path);
            if !dir.is_absolute() {
                return Err(AppError::Config(
                    "FONO_TEST_DATA_DIR must be absolute".into(),
                ));
            }
            std::fs::create_dir_all(&dir)?;
            return Ok(dir);
        }
    }
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

/// Automatic updater rate limiting is technical state, separate from user settings.
#[cfg(windows)]
pub(crate) fn load_update_check_time() -> AppResult<Option<u64>> {
    let _guard = PERSISTENCE_LOCK.lock();
    load_json_with_backup(&app_data_dir()?.join("update-check-time.json"))
}

#[cfg(windows)]
pub(crate) fn save_update_check_time(timestamp: u64) -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    save_json_atomically(&app_data_dir()?.join("update-check-time.json"), &timestamp)
}

pub fn history_path() -> AppResult<std::path::PathBuf> {
    Ok(app_data_dir()?.join("dictation-history.json"))
}

pub fn transcription_history_path() -> AppResult<std::path::PathBuf> {
    Ok(app_data_dir()?.join("transcription-history.json"))
}

/// Bearer token for clients of the loopback transcription service. It is kept
/// outside settings so renderer reads and settings exports never expose it.
pub fn transcription_api_token() -> AppResult<String> {
    let path = app_data_dir()?.join("api-token.txt");
    match fs::read_to_string(&path) {
        Ok(token) if !token.trim().is_empty() => Ok(token.trim().to_owned()),
        Ok(_) => {
            let token = uuid::Uuid::new_v4().simple().to_string();
            write_atomically(&path, token.as_bytes(), true)?;
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let token = uuid::Uuid::new_v4().simple().to_string();
            write_atomically(&path, token.as_bytes(), true)?;
            Ok(token)
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn load_history_document<T>() -> AppResult<Vec<T>>
where
    T: DeserializeOwned + Serialize,
{
    let path = history_path()?;
    let Some((entries, legacy)) = load_versioned_json_with_backup(&path)? else {
        return Ok(Vec::new());
    };
    if legacy {
        save_versioned_json_atomically(&path, &entries)?;
    }
    Ok(entries)
}

pub(crate) fn save_history_document<T>(entries: &[T]) -> AppResult<()>
where
    T: Serialize,
{
    save_versioned_json_atomically(&history_path()?, &entries)
}

pub(crate) fn load_transcription_history_document<T>() -> AppResult<Vec<T>>
where
    T: DeserializeOwned + Serialize,
{
    let path = transcription_history_path()?;
    let Some((entries, legacy)) = load_versioned_json_with_backup(&path)? else {
        return Ok(Vec::new());
    };
    if legacy {
        save_versioned_json_atomically(&path, &entries)?;
    }
    Ok(entries)
}

pub(crate) fn save_transcription_history_document<T>(entries: &[T]) -> AppResult<()>
where
    T: Serialize,
{
    save_versioned_json_atomically(&transcription_history_path()?, &entries)
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
    let Some((mut settings, legacy)) = load_versioned_json_with_backup::<Settings>(&path)? else {
        return Ok(None);
    };

    let normalized_mode = settings.enforce_classic_dictation();
    let normalized_wake = settings.enforce_wake_availability();
    let migrated_profiles = settings.migrate_llm_profiles();
    let migrated_prompts = settings.migrate_processing_prompts();
    let legacy_secret = if let Some(api_key) = settings.llm_api_key.take() {
        crate::secrets::store_llm_api_key(&api_key)?;
        Some(api_key)
    } else {
        crate::secrets::load_llm_api_key()?
    };

    if migrated_profiles {
        if let Some(api_key) = legacy_secret.as_deref() {
            crate::secrets::store_llm_profile_api_key(
                crate::types::LlmProfile::DEFAULT_ID,
                api_key,
            )?;
        }
    }
    for profile in &mut settings.llm_profiles {
        profile.api_key = None;
        profile.has_api_key = crate::secrets::load_llm_profile_api_key(&profile.id)?.is_some();
    }
    settings.has_llm_api_key = settings
        .llm_profile(Some(crate::types::LlmProfile::DEFAULT_ID))
        .is_some_and(|profile| profile.has_api_key);
    if legacy || migrated_profiles || normalized_mode || migrated_prompts || normalized_wake {
        save_versioned_json_atomically(&path, &settings)?;
    }
    {
        tracing::info!(?path, "loaded settings");
    }
    Ok(Some(settings))
}

pub fn save_settings(settings: &Settings) -> AppResult<()> {
    let _guard = PERSISTENCE_LOCK.lock();
    let path = settings_path()?;
    let mut settings = settings.clone();
    settings.enforce_classic_dictation();
    settings.enforce_wake_availability();
    settings.migrate_processing_prompts();
    save_versioned_json_atomically(&path, &settings)?;
    tracing::info!(?path, "saved settings");
    Ok(())
}

fn load_versioned_json_with_backup<T>(path: &Path) -> AppResult<Option<(T, bool)>>
where
    T: DeserializeOwned + Serialize,
{
    let Some(raw) = load_json_with_backup::<serde_json::Value>(path)? else {
        return Ok(None);
    };
    if raw.get("schema_version").is_some() {
        let document: VersionedDocument<T> = serde_json::from_value(raw)?;
        if document.schema_version != PERSISTENCE_SCHEMA_VERSION {
            return Err(crate::error::AppError::Config(format!(
                "unsupported persistence schema version {} in {path:?}; expected {PERSISTENCE_SCHEMA_VERSION}",
                document.schema_version
            )));
        }
        Ok(Some((document.data, false)))
    } else {
        Ok(Some((serde_json::from_value(raw)?, true)))
    }
}

fn save_versioned_json_atomically<T>(path: &Path, value: &T) -> AppResult<()>
where
    T: Serialize,
{
    save_json_atomically(
        path,
        &VersionedDocument {
            schema_version: PERSISTENCE_SCHEMA_VERSION,
            data: value,
        },
    )
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

    #[test]
    fn legacy_document_is_detected_and_rewritten_with_schema_version() {
        let path = temporary_test_path("legacy.json");
        let legacy = StoredValue {
            value: "legacy".into(),
        };
        save_json_atomically(&path, &legacy).expect("write legacy document");

        let (loaded, needs_migration) = load_versioned_json_with_backup::<StoredValue>(&path)
            .expect("load legacy document")
            .expect("legacy document exists");
        assert_eq!(loaded, legacy);
        assert!(needs_migration);

        save_versioned_json_atomically(&path, &loaded).expect("write versioned document");
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read versioned document"))
                .expect("parse versioned document");
        assert_eq!(raw["schema_version"], PERSISTENCE_SCHEMA_VERSION);
        assert_eq!(raw["data"]["value"], "legacy");

        fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
    }

    #[test]
    fn future_schema_version_is_rejected() {
        let path = temporary_test_path("future.json");
        fs::write(&path, r#"{"schema_version":999,"data":{"value":"future"}}"#)
            .expect("write future document");

        let error = load_versioned_json_with_backup::<StoredValue>(&path).unwrap_err();
        assert!(error.to_string().contains("unsupported persistence schema"));

        fs::remove_dir_all(path.parent().expect("test directory")).expect("remove test directory");
    }

    #[test]
    fn command_proposal_is_invalidated_when_settings_change() {
        let state = AppState::new();
        let proposal = crate::application::command_proposal::create(
            &state,
            7,
            crate::operation::OperationSource::Hotkey,
            "громче".into(),
            None,
        );
        assert!(state.pending_command_proposal().is_some());

        state.set_settings(Settings::default());
        let error = state.take_pending_command_proposal().unwrap_err();
        assert!(error.to_string().contains("invalidated"));
        assert_eq!(proposal.operation_id, 7);
    }

    #[test]
    fn expired_command_proposal_cannot_be_confirmed() {
        let state = AppState::new();
        let mut proposal = crate::application::command_proposal::create(
            &state,
            8,
            crate::operation::OperationSource::Hotkey,
            "тише".into(),
            None,
        );
        proposal.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
        state.set_pending_command_proposal(Some(proposal));

        assert!(state.pending_command_proposal().is_none());
        assert!(state.take_pending_command_proposal().is_err());
    }
}
