//! Verified downloadable artifacts; backends without a runtime stay out of this list.
use crate::{
    error::AppResult,
    state,
    types::{ModelId, WhisperModelSize},
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ModelCapabilities {
    pub transcription: bool,
    pub initial_prompt: bool,
    pub language_detection: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SttModelInfo {
    pub id: ModelId,
    pub filename: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub license: String,
    pub license_url: String,
    /// Fono's exposed languages; multilingual marks wider upstream support.
    pub languages: Vec<String>,
    pub multilingual: bool,
    pub punctuation: bool,
    pub capabilities: ModelCapabilities,
    pub local_path: Option<String>,
    pub installed_bytes: Option<u64>,
}

pub fn describe(size: WhisperModelSize) -> SttModelInfo {
    SttModelInfo {
        id: ModelId::whisper(size),
        filename: size.filename().into(),
        url: size.url().into(),
        sha256: size.sha256().into(),
        bytes: size.exact_bytes(),
        license: "MIT".into(),
        license_url: "https://github.com/openai/whisper/blob/main/LICENSE".into(),
        languages: vec!["ru".into(), "en".into()],
        multilingual: true,
        punctuation: true,
        capabilities: ModelCapabilities {
            transcription: true,
            initial_prompt: true,
            language_detection: true,
        },
        local_path: None,
        installed_bytes: None,
    }
}

pub fn list_stt_models() -> AppResult<Vec<SttModelInfo>> {
    let directory = state::models_dir()?;
    Ok(WhisperModelSize::ALL
        .into_iter()
        .map(|size| {
            let mut info = describe(size);
            let path = directory.join(size.filename());
            if let Some(metadata) = path.metadata().ok().filter(|metadata| metadata.is_file()) {
                info.local_path = Some(path.to_string_lossy().into_owned());
                info.installed_bytes = Some(metadata.len());
            }
            info
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_verified_artifacts_and_explicit_metadata() {
        let mut ids = std::collections::HashSet::new();
        for entry in WhisperModelSize::ALL.into_iter().map(describe) {
            assert!(ids.insert(serde_json::to_string(&entry.id).unwrap()));
            assert!(entry
                .url
                .starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/"));
            assert_eq!(entry.sha256.len(), 64);
            assert!(entry.sha256.bytes().all(|value| value.is_ascii_hexdigit()));
            assert!(entry.bytes > 0);
            assert_eq!(entry.license, "MIT");
            assert_eq!(entry.languages, ["ru", "en"]);
            assert!(entry.multilingual && entry.punctuation);
            assert!(entry.capabilities.transcription && entry.capabilities.initial_prompt);
        }
    }
}
