//! Глобальное состояние приложения.
//!
//! Хранится в `tauri::State` и доступно из всех команд и модулей.
//! Содержит текущие настройки, состояние конвейера и закэшированные
//! ресурсы (загруженная whisper-модель, активный аудио-поток).

use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::AppResult;
use crate::types::DictationHistoryEntry;
use crate::types::Settings;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pipeline_state: Mutex<crate::types::PipelineState>,
    pub dictation_paused: Mutex<bool>,
    pending_voice_command: Mutex<Option<String>>,
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
        if let Some(s) = load_settings().unwrap_or(None) {
            *state.settings.lock() = s;
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
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

pub fn append_dictation_history(entry: DictationHistoryEntry) -> AppResult<()> {
    let mut entries = load_dictation_history()?;
    entries.insert(0, entry);
    entries.truncate(200);
    std::fs::write(history_path()?, serde_json::to_string_pretty(&entries)?)?;
    Ok(())
}

pub fn clear_dictation_history() -> AppResult<()> {
    std::fs::write(history_path()?, "[]")?;
    Ok(())
}

pub fn delete_dictation_history_entry(id: &str) -> AppResult<()> {
    let mut entries = load_dictation_history()?;
    entries.retain(|entry| entry.id != id);
    std::fs::write(history_path()?, serde_json::to_string_pretty(&entries)?)?;
    Ok(())
}

/// Каталог для whisper-моделей.
pub fn models_dir() -> AppResult<std::path::PathBuf> {
    let dir = app_data_dir()?.join("whisper-models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn load_settings() -> AppResult<Option<Settings>> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let s: Settings = serde_json::from_str(&raw)?;
    tracing::info!("loaded settings from {:?}", path);
    Ok(Some(s))
}

pub fn save_settings(settings: &Settings) -> AppResult<()> {
    let path = settings_path()?;
    let raw = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, raw)?;
    tracing::info!("saved settings to {:?}", path);
    Ok(())
}

/// Хранилище разделяемых аудио-чанков.
/// Продюсер = аудио-поток cpal, консьюмер = pipeline / wakeword / vad.
pub type AudioBuffer = Arc<Mutex<Vec<i16>>>;
