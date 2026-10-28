//! Backend preference values; the caller supplies its compiled capabilities.

use serde::{Deserialize, Serialize};

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
    pub fn use_gpu(self, capabilities: AccelerationCapabilities) -> bool {
        match self {
            Self::Cpu => false,
            Self::Auto => capabilities.cuda || capabilities.vulkan,
            Self::Cuda => capabilities.cuda,
            Self::Vulkan => capabilities.vulkan,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AccelerationCapabilities {
    pub cuda: bool,
    pub vulkan: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_uses_the_callers_capabilities_instead_of_crate_features() {
        let cpu = AccelerationCapabilities {
            cuda: false,
            vulkan: false,
        };
        let cuda = AccelerationCapabilities {
            cuda: true,
            vulkan: false,
        };
        let vulkan = AccelerationCapabilities {
            cuda: false,
            vulkan: true,
        };
        assert!(!AccelerationMode::Auto.use_gpu(cpu));
        assert!(AccelerationMode::Auto.use_gpu(cuda));
        assert!(AccelerationMode::Auto.use_gpu(vulkan));
        assert!(AccelerationMode::Cuda.use_gpu(cuda));
        assert!(!AccelerationMode::Cuda.use_gpu(vulkan));
        assert!(AccelerationMode::Vulkan.use_gpu(vulkan));
        assert!(!AccelerationMode::Vulkan.use_gpu(cuda));
        assert!(!AccelerationMode::Cpu.use_gpu(cuda));
        assert!(!AccelerationMode::Cpu.use_gpu(vulkan));
    }
}
