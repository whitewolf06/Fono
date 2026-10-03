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
pub const SILERO_VAD_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
pub const SILERO_VAD_SHA256: &str =
    "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
pub const KWS_MODEL: WakeModelSpec = WakeModelSpec {
    directory: "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01",
    archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2",
    archive_sha256: "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a",
    archive_bytes: 17_626_723,
    sample_rate: 16_000,
    files: &[
        "encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "tokens.txt",
    ],
    license_url: "https://github.com/k2-fsa/icefall/blob/master/LICENSE",
};
pub fn model_spec(backend: WakeWordBackend) -> Option<&'static WakeModelSpec> {
    match backend {
        WakeWordBackend::SherpaOnnx => Some(&KWS_MODEL),
        WakeWordBackend::SherpaStreamingRu => Some(&RU_MODEL),
        WakeWordBackend::SherpaStreamingEn => Some(&EN_MODEL),
        _ => None,
    }
}
pub fn model_version(backend: WakeWordBackend) -> &'static str {
    model_spec(backend)
        .map(|s| s.directory)
        .unwrap_or("sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01")
}
