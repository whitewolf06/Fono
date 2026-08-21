//! Типы, общие между Rust и TypeScript.
//!
//! Эти структуры зеркалируют `src/lib/types.ts` на фронтенде.
//! При изменении не забудьте синхронизировать обе стороны.

pub use fono_wake::WakeWordBackend;
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
    Injecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    /// Full-size multilingual Whisper Large v3 model.
    Large,
    /// Faster distilled variant of Large v3. It trades some accuracy for
    /// noticeably lower memory use and latency.
    LargeTurbo,
}

impl WhisperModelSize {
    pub const ALL: [Self; 6] = [
        Self::Tiny,
        Self::Base,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::LargeTurbo,
    ];

    pub fn filename(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "ggml-tiny.bin",
            WhisperModelSize::Base => "ggml-base.bin",
            WhisperModelSize::Small => "ggml-small.bin",
            WhisperModelSize::Medium => "ggml-medium.bin",
            WhisperModelSize::Large => "ggml-large-v3.bin",
            WhisperModelSize::LargeTurbo => "ggml-large-v3-turbo.bin",
        }
    }

    pub fn url(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
            }
            WhisperModelSize::Base => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
            }
            WhisperModelSize::Small => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
            }
            WhisperModelSize::Medium => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin"
            }
            WhisperModelSize::Large => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin"
            }
            WhisperModelSize::LargeTurbo => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
            }
        }
    }

    pub fn approx_bytes(&self) -> u64 {
        (match self {
            WhisperModelSize::Tiny => 77,
            WhisperModelSize::Base => 147,
            WhisperModelSize::Small => 488,
            WhisperModelSize::Medium => 1530,
            WhisperModelSize::Large => 3010,
            WhisperModelSize::LargeTurbo => 1549,
        }) * 1024
            * 1024
    }

    /// SHA-256 published for the exact files behind the Hugging Face URLs.
    /// Source: https://huggingface.co/ggerganov/whisper.cpp/tree/main
    pub fn sha256(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => {
                "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
            }
            WhisperModelSize::Base => {
                "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
            }
            WhisperModelSize::Small => {
                "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
            }
            WhisperModelSize::Medium => {
                "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"
            }
            WhisperModelSize::Large => {
                "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2"
            }
            WhisperModelSize::LargeTurbo => {
                "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
            }
        }
    }
}

#[cfg(test)]
mod whisper_model_size_tests {
    use super::WhisperModelSize;

    #[test]
    fn model_catalog_contains_the_two_large_v3_variants() {
        assert_eq!(WhisperModelSize::ALL.len(), 6);
        assert_eq!(WhisperModelSize::Large.filename(), "ggml-large-v3.bin");
        assert_eq!(
            WhisperModelSize::LargeTurbo.filename(),
            "ggml-large-v3-turbo.bin"
        );
        assert!(
            WhisperModelSize::Large.approx_bytes() > WhisperModelSize::LargeTurbo.approx_bytes()
        );
    }

    #[test]
    fn large_turbo_uses_a_stable_ipc_value_and_verification_hash() {
        assert_eq!(
            serde_json::to_string(&WhisperModelSize::LargeTurbo).unwrap(),
            "\"large_turbo\""
        );
        assert_eq!(
            serde_json::from_str::<WhisperModelSize>("\"large\"").unwrap(),
            WhisperModelSize::Large
        );
        assert_eq!(
            WhisperModelSize::LargeTurbo.sha256(),
            "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperModelInfo {
    pub filename: String,
    pub size: WhisperModelSize,
    /// `Some(path)` если модель уже скачана локально.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMode {
    Off,
    Clean,
    Format,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InjectionMode {
    SendInput,
    Clipboard,
}

/// Preferred Whisper acceleration. `Auto` uses the GPU backend compiled into
/// this release (CUDA or Vulkan) and falls back to CPU when there is none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccelerationMode {
    #[default]
    Auto,
    Cuda,
    Vulkan,
    Cpu,
}

impl AccelerationMode {
    pub fn use_gpu(self) -> bool {
        match self {
            Self::Cpu => false,
            Self::Auto => cfg!(any(feature = "cuda", feature = "vulkan")),
            Self::Cuda => cfg!(feature = "cuda"),
            Self::Vulkan => cfg!(feature = "vulkan"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AccelerationCapabilities {
    pub cuda: bool,
    pub vulkan: bool,
}

pub fn acceleration_capabilities() -> AccelerationCapabilities {
    AccelerationCapabilities {
        cuda: cfg!(feature = "cuda"),
        vulkan: cfg!(feature = "vulkan"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    LmStudio,
    OpenAi,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub audio_device_id: Option<String>,
    #[serde(default)]
    pub whisper_model_path: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub wake_word_enabled: bool,
    #[serde(default = "default_wake_word")]
    pub wake_word: String,
    #[serde(default = "default_wake_backend")]
    pub wake_backend: WakeWordBackend,
    #[serde(default = "default_wake_word_threshold")]
    pub wake_word_threshold: f32,
    #[serde(default = "default_wake_word_sensitivity")]
    pub wake_word_sensitivity: f32,
    #[serde(default = "default_ai_mode")]
    pub ai_mode: AiMode,
    #[serde(default = "default_llm_url")]
    pub llm_base_url: String,
    #[serde(default)]
    pub llm_model: Option<String>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub overlay_x: Option<i32>,
    #[serde(default)]
    pub overlay_y: Option<i32>,
    #[serde(default)]
    pub clean_prompt: Option<String>,
    /// Legacy field read from existing settings.json files. New settings use
    /// `acceleration`; keep it out of IPC and future writes.
    #[serde(skip_serializing, default = "default_use_gpu")]
    pub use_gpu: bool,
    #[serde(default)]
    pub acceleration: AccelerationMode,
    #[serde(default = "default_injection_mode")]
    pub injection_mode: InjectionMode,
    #[serde(default = "default_command_hotkey")]
    pub command_hotkey: String,
    #[serde(default)]
    pub launch_apps: Vec<LaunchApp>,
    #[serde(default = "default_overlay_scale")]
    pub overlay_scale: f32,
    #[serde(default = "default_overlay_opacity")]
    pub overlay_opacity: f32,
    #[serde(default)]
    pub overlay_mini_mode: bool,
    #[serde(default)]
    pub verbose_logging: bool,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: LlmProvider,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub llm_api_key: Option<String>,
    #[serde(default)]
    pub has_llm_api_key: bool,
    #[serde(default = "default_history_enabled")]
    pub history_enabled: bool,
    #[serde(default = "default_wake_word_model")]
    pub wake_word_model: WhisperModelSize,
    #[serde(default = "default_wake_word_vad_threshold")]
    pub wake_word_vad_threshold: f32,
    /// Сколько тишины после речи ждать перед завершением диктовки по wake word.
    #[serde(default = "default_wake_dictation_silence_ms")]
    pub wake_dictation_silence_ms: u64,
    /// RMS-порог, ниже которого диктовка после wake word считает звук тишиной.
    #[serde(default = "default_wake_dictation_speech_threshold")]
    pub wake_dictation_speech_threshold: f32,
    #[serde(default = "default_volume_step")]
    pub volume_step: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchApp {
    pub name: String,
    pub exe_path: String,
    pub aliases: Vec<String>,
}

/// Immutable settings required to execute a confirmed voice-command proposal.
/// Secrets and unrelated UI settings are deliberately excluded.
#[derive(Debug, Clone)]
pub struct CommandSettingsSnapshot {
    pub version: u64,
    pub launch_apps: Vec<LaunchApp>,
    pub volume_step: u32,
}

/// A command is always previewed before it may affect another application.
/// The payload is serialisable so the renderer can show the proposal without
/// reaching into application state.
#[derive(Debug, Clone, Serialize)]
pub struct CommandProposal {
    pub id: u64,
    pub operation_id: u64,
    pub source: crate::operation::OperationSource,
    pub original_text: String,
    pub normalized_action: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub settings_version: u64,
    #[serde(skip)]
    pub settings_snapshot: CommandSettingsSnapshot,
}

impl CommandProposal {
    pub fn is_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now >= self.expires_at
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio_device_id: None,
            whisper_model_path: None,
            language: default_language(),
            hotkey: default_hotkey(),
            wake_word_enabled: false,
            wake_word: default_wake_word(),
            wake_backend: default_wake_backend(),
            wake_word_threshold: default_wake_word_threshold(),
            wake_word_sensitivity: default_wake_word_sensitivity(),
            ai_mode: default_ai_mode(),
            llm_base_url: default_llm_url(),
            llm_model: None,
            autostart: false,
            overlay_x: None,
            overlay_y: None,
            clean_prompt: None,
            use_gpu: default_use_gpu(),
            acceleration: AccelerationMode::Auto,
            injection_mode: default_injection_mode(),
            command_hotkey: default_command_hotkey(),
            launch_apps: Vec::new(),
            overlay_scale: default_overlay_scale(),
            overlay_opacity: default_overlay_opacity(),
            overlay_mini_mode: false,
            verbose_logging: false,
            llm_provider: default_llm_provider(),
            llm_api_key: None,
            has_llm_api_key: false,
            history_enabled: default_history_enabled(),
            wake_word_model: default_wake_word_model(),
            wake_word_vad_threshold: default_wake_word_vad_threshold(),
            wake_dictation_silence_ms: default_wake_dictation_silence_ms(),
            wake_dictation_speech_threshold: default_wake_dictation_speech_threshold(),
            volume_step: default_volume_step(),
        }
    }
}

fn default_use_gpu() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

fn default_history_enabled() -> bool {
    true
}

fn default_injection_mode() -> InjectionMode {
    InjectionMode::SendInput
}

fn default_command_hotkey() -> String {
    "Ctrl+Shift+Space".to_string()
}

fn default_overlay_scale() -> f32 {
    1.0
}

fn default_overlay_opacity() -> f32 {
    1.0
}

fn default_llm_provider() -> LlmProvider {
    LlmProvider::LmStudio
}

fn default_wake_word_model() -> WhisperModelSize {
    WhisperModelSize::Base
}

fn default_wake_backend() -> WakeWordBackend {
    WakeWordBackend::SherpaOnnx
}

fn default_wake_word_threshold() -> f32 {
    0.25
}

fn default_wake_word_sensitivity() -> f32 {
    0.5
}

fn default_wake_word_vad_threshold() -> f32 {
    0.015
}

fn default_wake_dictation_silence_ms() -> u64 {
    2_000
}

fn default_wake_dictation_speech_threshold() -> f32 {
    0.006
}

fn default_volume_step() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub detected_language: Option<String>,
    /// Время транскрипции в секундах (для отображения в UI).
    #[serde(default)]
    pub transcribe_secs: Option<f32>,
    /// Длительность аудио в секундах.
    #[serde(default)]
    pub audio_secs: Option<f32>,
    /// Какое устройство использовалось (CPU/CUDA).
    #[serde(default)]
    pub device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationHistoryEntry {
    pub id: String,
    pub text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub device: Option<String>,
}

fn default_language() -> String {
    "auto".to_string()
}
fn default_hotkey() -> String {
    "Ctrl+Space".to_string()
}
fn default_wake_word() -> String {
    "hey fono".to_string()
}
fn default_ai_mode() -> AiMode {
    AiMode::Clean
}
fn default_llm_url() -> String {
    "http://localhost:1234/v1".to_string()
}
