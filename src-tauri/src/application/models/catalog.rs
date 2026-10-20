//! Verified downloadable artifacts; backends without a runtime stay out of this list.
use crate::{error::AppResult, state, types::WhisperArtifact};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ModelCapabilities {
    pub transcription: bool,
    pub initial_prompt: bool,
    pub hotwords: bool,
    pub language_detection: bool,
    pub language_selection: bool,
    pub decoding_options: bool,
    pub accelerations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SttModelInfo {
    pub id: crate::types::ModelId,
    pub filename: String,
    pub url: String,
    pub sha256: Option<String>,
    pub files: Vec<crate::stt::offline_models::OfflineArtifact>,
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

pub fn describe(artifact: WhisperArtifact) -> SttModelInfo {
    SttModelInfo {
        id: artifact.id(),
        filename: artifact.filename,
        url: artifact.url,
        sha256: Some(artifact.sha256),
        files: Vec::new(),
        bytes: artifact.bytes,
        license: "MIT".into(),
        license_url: "https://github.com/openai/whisper/blob/main/LICENSE".into(),
        languages: vec!["ru".into(), "en".into()],
        multilingual: true,
        punctuation: true,
        capabilities: ModelCapabilities {
            transcription: true,
            initial_prompt: true,
            hotwords: false,
            language_detection: true,
            language_selection: true,
            decoding_options: true,
            accelerations: vec!["auto".into(), "cpu".into(), "cuda".into(), "vulkan".into()],
        },
        local_path: None,
        installed_bytes: None,
    }
}

pub fn list_stt_models() -> AppResult<Vec<SttModelInfo>> {
    let directory = state::models_dir()?;
    let models: Vec<_> = WhisperArtifact::all()?
        .into_iter()
        .map(|artifact| {
            let path = directory.join(&artifact.filename);
            let mut info = describe(artifact);
            if let Some(metadata) = path.metadata().ok().filter(|metadata| metadata.is_file()) {
                info.local_path = Some(path.to_string_lossy().into_owned());
                info.installed_bytes = Some(metadata.len());
            }
            info
        })
        .collect();
    #[cfg(feature = "offline-stt")]
    let mut models = models;
    #[cfg(feature = "offline-stt")]
    for spec in crate::stt::offline_models::specifications()? {
        let path = directory.join(&spec.directory);
        let installed = super::offline_package::is_installed(&path, &spec);
        let bytes = spec.files.iter().map(|file| file.bytes).sum();
        models.push(SttModelInfo {
            id: spec.id,
            filename: spec.directory,
            url: format!(
                "https://huggingface.co/{}/tree/{}",
                spec.repository, spec.revision
            ),
            sha256: None,
            files: spec.files,
            bytes,
            license: spec.license,
            license_url: spec.license_url,
            languages: spec.languages,
            multilingual: false,
            punctuation: spec.punctuation,
            capabilities: ModelCapabilities {
                transcription: true,
                initial_prompt: false,
                hotwords: false,
                language_detection: false,
                language_selection: false,
                decoding_options: false,
                accelerations: vec!["auto".into(), "cpu".into()],
            },
            local_path: installed.then(|| path.to_string_lossy().into_owned()),
            installed_bytes: installed.then_some(bytes),
        });
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_verified_artifacts_and_explicit_metadata() {
        let mut ids = std::collections::HashSet::new();
        for entry in WhisperArtifact::all().unwrap().into_iter().map(describe) {
            assert!(ids.insert(serde_json::to_string(&entry.id).unwrap()));
            assert!(entry
                .url
                .starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/"));
            let digest = entry.sha256.unwrap();
            assert_eq!(digest.len(), 64);
            assert!(digest.bytes().all(|value| value.is_ascii_hexdigit()));
            assert!(entry.bytes > 0);
            assert_eq!(entry.license, "MIT");
            assert_eq!(entry.languages, ["ru", "en"]);
            assert!(entry.multilingual && entry.punctuation);
            assert!(entry.capabilities.transcription && entry.capabilities.initial_prompt);
        }
    }
}
