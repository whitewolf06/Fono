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
#[serde(rename_all = "lowercase")]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

impl WhisperModelSize {
    pub fn filename(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "ggml-tiny.bin",
            WhisperModelSize::Base => "ggml-base.bin",
            WhisperModelSize::Small => "ggml-small.bin",
            WhisperModelSize::Medium => "ggml-medium.bin",
            WhisperModelSize::Large => "ggml-large-v3.bin",
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
        }
    }

    pub fn approx_bytes(&self) -> u64 {
        (match self {
            WhisperModelSize::Tiny => 77,
            WhisperModelSize::Base => 147,
            WhisperModelSize::Small => 488,
            WhisperModelSize::Medium => 1530,
            WhisperModelSize::Large => 3010,
        }) * 1024
            * 1024
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
    #[serde(default = "default_use_gpu")]
    pub use_gpu: bool,
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
    pub llm_api_key: Option<String>,
    #[serde(default = "default_wake_word_model")]
    pub wake_word_model: WhisperModelSize,
    #[serde(default = "default_wake_word_vad_threshold")]
    pub wake_word_vad_threshold: f32,
    #[serde(default = "default_volume_step")]
    pub volume_step: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchApp {
    pub name: String,
    pub exe_path: String,
    pub aliases: Vec<String>,
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
            injection_mode: default_injection_mode(),
            command_hotkey: default_command_hotkey(),
            launch_apps: Vec::new(),
            overlay_scale: default_overlay_scale(),
            overlay_opacity: default_overlay_opacity(),
            overlay_mini_mode: false,
            verbose_logging: false,
            llm_provider: default_llm_provider(),
            llm_api_key: None,
            wake_word_model: default_wake_word_model(),
            wake_word_vad_threshold: default_wake_word_vad_threshold(),
            volume_step: default_volume_step(),
        }
    }
}

fn default_use_gpu() -> bool {
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
    0.5
}

fn default_wake_word_sensitivity() -> f32 {
    0.5
}

fn default_wake_word_vad_threshold() -> f32 {
    0.015
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
