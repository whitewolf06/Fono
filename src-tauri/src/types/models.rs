//! Audio devices, local Whisper models and acceleration selection.

pub use fono_stt_protocol::{DecodingOptions, DecodingStrategy};
use serde::{Deserialize, Serialize};

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

    /// Stable public identifier used by the local transcription API.
    pub fn api_identifier(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "tiny",
            WhisperModelSize::Base => "base",
            WhisperModelSize::Small => "small",
            WhisperModelSize::Medium => "medium",
            WhisperModelSize::Large => "large",
            WhisperModelSize::LargeTurbo => "large_turbo",
        }
    }

    pub fn from_filename(filename: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|model| model.filename() == filename)
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

    /// Exact GGML file sizes from upstream LFS pointers (F-01.1).
    pub fn exact_bytes(&self) -> u64 {
        match self {
            Self::Tiny => 77_691_713,
            Self::Base => 147_951_465,
            Self::Small => 487_601_967,
            Self::Medium => 1_533_763_059,
            Self::Large => 3_095_033_483,
            Self::LargeTurbo => 1_624_555_275,
        }
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

    #[test]
    fn public_api_identifiers_are_derived_from_known_model_filenames() {
        assert_eq!(
            WhisperModelSize::from_filename("ggml-large-v3-turbo.bin")
                .map(|model| model.api_identifier()),
            Some("large_turbo")
        );
        assert!(WhisperModelSize::from_filename("custom-model.bin").is_none());
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

/// GPU residency is independent of the selected inference backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuModelResidency {
    #[default]
    Resident,
    Adaptive,
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
