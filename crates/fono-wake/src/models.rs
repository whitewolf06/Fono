use crate::WakeWordBackend;

/// Pinned upstream distributions. Archive hashes are verified before extraction.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeModelSpec {
    pub directory: &'static str,
    pub archive_url: &'static str,
    pub archive_sha256: &'static str,
    pub archive_bytes: u64,
    pub sample_rate: u32,
    pub files: &'static [&'static str],
    pub license_url: &'static str,
}
pub const RU_MODEL: WakeModelSpec = WakeModelSpec {
    directory: "sherpa-onnx-streaming-t-one-russian-2025-09-08",
    archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-t-one-russian-2025-09-08.tar.bz2",
    archive_sha256: "b9c907450e99a6e5049e279bf18368a17db0bdc5e63b7fa978943138debbe3ae",
    archive_bytes: 128_468_156,
    sample_rate: 8_000,
    files: &["model.onnx", "tokens.txt", "LICENSE"],
    license_url: "https://github.com/voicekit-team/T-one/blob/main/LICENSE",
};
pub const EN_MODEL: WakeModelSpec = WakeModelSpec {
    directory: "sherpa-onnx-streaming-zipformer-en-2023-06-26",
    archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2",
    archive_sha256: "639e25b578e9e997131402199419c13a941f8e4e198e2da1ce57dbf5cf401282",
    archive_bytes: 310_414_022,
    sample_rate: 16_000,
    files: &[
        "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        "decoder-epoch-99-avg-1-chunk-16-left-128.onnx",
        "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        "tokens.txt",
    ],
    license_url: "https://github.com/k2-fsa/icefall/blob/master/LICENSE",
};
pub fn model_spec(backend: WakeWordBackend) -> Option<&'static WakeModelSpec> {
    match backend {
        WakeWordBackend::SherpaStreamingRu => Some(&RU_MODEL),
        WakeWordBackend::SherpaStreamingEn => Some(&EN_MODEL),
        _ => None,
    }
}
pub fn model_version(backend: WakeWordBackend) -> &'static str {
    model_spec(backend)
        .map(|s| s.directory)
        .unwrap_or("retired")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_backends_have_no_downloadable_model_or_legacy_fallback() {
        for backend in [
            WakeWordBackend::SherpaOnnx,
            WakeWordBackend::WhisperExperimental,
        ] {
            assert!(model_spec(backend).is_none());
            assert_eq!(model_version(backend), "retired");
        }
    }
}
