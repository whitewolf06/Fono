//! Глобальное состояние приложения.
//!
//! Хранится в `tauri::State` и доступно из всех команд и модулей.
//! Содержит подтверждённые настройки, состояние конвейера и последнюю расшифровку.
//! Аудиозахват, модели и ресурсы активных операций принадлежат своим сервисам.

mod persistence;
mod settings_delta;
pub use persistence::{
    app_data_dir, load_settings, models_dir, save_settings, settings_path, transcription_api_token,
    transcription_history_path,
};
pub(crate) use persistence::{
    delete_legacy_document, load_legacy_history_at, load_legacy_trainer_cache_at,
    load_trainer_cache_document, load_transcription_history_document, purge_trainer_cache_document,
    save_history_migration_backup, save_trainer_cache_document,
    save_transcription_history_document,
};
#[cfg(windows)]
pub(crate) use persistence::{load_update_check_time, save_update_check_time};

use crate::error::{AppError, AppResult};
use crate::types::{CommandProposal, CommandSettingsSnapshot, Settings};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

pub struct AppState {
    pub settings: Mutex<Settings>,
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

    pub fn settings_revision(&self) -> u64 {
        self.settings_version.load(Ordering::SeqCst)
    }

    pub fn set_settings(&self, mut settings: Settings) {
        // This infallible setter is used after validated settings transactions.
        // Preserve previous state if an internal caller supplies an unavailable engine.
        if let Err(error) = settings.reconcile_stt_model(None) {
            tracing::error!(%error, "invalid STT model selection");
            return;
        }
        settings.enforce_classic_dictation();
        settings.enforce_wake_availability();
        settings.migrate_processing_prompts();
        {
            let mut current = self.settings.lock();
            *current = settings;
            self.settings_version.fetch_add(1, Ordering::SeqCst);
        }
        crate::application::trainer_ai::settings_changed();
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

/// Shared audio chunks owned by capture/pipeline services.
/// Shared audio chunks owned by capture/pipeline services.
pub type AudioBuffer = Arc<Mutex<Vec<i16>>>;
