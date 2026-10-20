//! Serialization defaults shared by the preference DTOs.

use super::{AiMode, InjectionMode, LlmProvider, WakeWordBackend, WhisperModelSize};
pub(super) fn default_selection_hotkey() -> String {
    "Ctrl+Alt+Space".into()
}

pub(super) fn default_use_gpu() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

pub(super) fn default_history_enabled() -> bool {
    true
}

pub(super) fn default_speech_trainer_enabled() -> bool {
    true
}

pub(super) fn default_history_retention_days() -> u16 {
    365
}

pub(super) fn default_analytics_retention_days() -> u16 {
    30
}

pub(super) fn default_injection_mode() -> InjectionMode {
    InjectionMode::SendInput
}

pub(super) fn default_command_hotkey() -> String {
    "Ctrl+Shift+Space".to_string()
}

pub(super) fn default_overlay_scale() -> f32 {
    1.0
}

pub(super) fn default_overlay_enabled() -> bool {
    true
}

pub(super) fn default_processing_translation_enabled() -> bool {
    true
}

pub(super) fn default_overlay_opacity() -> f32 {
    1.0
}

pub(super) fn default_llm_provider() -> LlmProvider {
    LlmProvider::LmStudio
}

pub(super) fn default_wake_word_model() -> WhisperModelSize {
    WhisperModelSize::Base
}

pub(super) fn default_wake_backend() -> WakeWordBackend {
    WakeWordBackend::SherpaOnnx
}

pub(super) fn default_wake_word_threshold() -> f32 {
    0.25
}

pub(super) fn default_wake_word_sensitivity() -> f32 {
    0.5
}

pub(super) fn default_wake_word_vad_threshold() -> f32 {
    0.015
}

pub(super) fn default_wake_dictation_silence_ms() -> u64 {
    2_000
}

pub(super) fn default_wake_dictation_speech_threshold() -> f32 {
    0.006
}

pub(super) fn default_volume_step() -> u32 {
    10
}

pub(super) fn default_language() -> String {
    "auto".to_string()
}
pub(super) fn default_hotkey() -> String {
    "Ctrl+Space".to_string()
}
pub(super) fn default_wake_word() -> String {
    "hey fono".to_string()
}
pub(super) fn default_ai_mode() -> AiMode {
    AiMode::Clean
}
pub(super) fn default_llm_url() -> String {
    "http://localhost:1234/v1".to_string()
}
