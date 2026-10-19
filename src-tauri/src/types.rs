//! Типы, общие между Rust и TypeScript.
//!
//! Эти структуры зеркалируют `src/v3/shared/infrastructure/native/ipcTypes.ts`.
//! При изменении не забудьте синхронизировать обе стороны.

pub use crate::application::dictation::workflow::{
    ProcessingWorkflow, TextPreset, TranslationLanguage,
};
mod wake_compat;
pub use wake_compat::{WakeWordBackend, WakeWordStatus};
mod history_metadata;
mod processing_prompts;
mod wake_availability;
pub use history_metadata::{
    DictationBackend, DictationHistoryMetadata, DictationTimingMeasurements,
};
pub use processing_prompts::{
    ProcessingPromptChoice, ProcessingPrompts, MAX_PROCESSING_PROMPT_CHARS,
    MAX_PROCESSING_TEXT_BYTES,
};
pub use wake_availability::{ensure_wake_available, WAKE_WORD_AVAILABLE, WAKE_WORD_UNAVAILABLE};

mod commands;
mod defaults;
mod history;
mod models;
mod processing;
mod settings;
mod stt_models;
mod wake_profile;
pub use commands::*;
pub use history::*;
pub use models::*;
pub use processing::*;
pub use settings::*;
pub use stt_models::*;
pub use wake_profile::*;

use serde::{Deserialize, Serialize};

/// Состояние голосового конвейера (FSM).
/// См. `docs/architecture.md` → "Основной конвейер".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    Idle,
    Listening,
    Transcribing,
    Processing,
    #[serde(rename = "awaiting_action")]
    AwaitingAction,
    Injecting,
    Error,
}
