//! Pinned, mutually compatible encoder/decoder/joiner/token packages.
#[cfg(feature = "offline-stt")]
use crate::types::SttModelSelection;
use crate::{
    error::{AppError, AppResult},
    types::ModelId,
};
use fono_core::OperationCancellation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineArtifact {
    pub filename: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineModelSpec {
    pub id: ModelId,
    pub directory: String,
    pub repository: String,
    pub revision: String,
    pub license: String,
    pub license_url: String,
    pub languages: Vec<String>,
    pub punctuation: bool,
    pub feature_dim: i32,
    pub encoder: String,
    pub decoder: String,
    pub joiner: String,
    pub tokens: String,
    pub files: Vec<OfflineArtifact>,
}

pub fn specifications() -> AppResult<Vec<OfflineModelSpec>> {
    serde_json::from_str(include_str!("offline_models.json"))
        .map_err(|_| AppError::Config("Invalid built-in offline model manifest".into()))
}

pub fn specification(id: &ModelId) -> AppResult<OfflineModelSpec> {
    specifications()?
        .into_iter()
        .find(|spec| &spec.id == id)
        .ok_or_else(|| AppError::Config("Unknown or unavailable offline model variant".into()))
}

#[cfg(feature = "offline-stt")]
pub(super) fn validate_selection(model: &SttModelSelection) -> AppResult<()> {
    if model.local_path.trim().is_empty() {
        return Err(AppError::Config("STT model directory is empty".into()));
    }
    specification(&model.id).map(|_| ())
}

/// On Windows these read-only handles deny writes/deletes for the complete
/// loaded runtime lifetime, keeping the checked model files stable for ORT.
pub fn verify_directory(
    directory: &Path,
    spec: &OfflineModelSpec,
    cancellation: &OperationCancellation,
) -> AppResult<Vec<File>> {
    let mut guards = Vec::new();
    for artifact in &spec.files {
        super::recognizer::check_cancellation(cancellation)?;
        if artifact.filename.is_empty()
            || artifact.filename.contains(['/', '\\', ':'])
            || artifact.filename == "."
            || artifact.filename == ".."
        {
            return Err(AppError::Config("Invalid offline artifact filename".into()));
        }
        let path = directory.join(&artifact.filename);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(AppError::Config(
                "Offline model artifacts must be regular files".into(),
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // FILE_SHARE_READ only
        }
        let mut file = options.open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != artifact.bytes {
            return Err(AppError::Config(
                "Offline model artifact size mismatch".into(),
            ));
        }
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            super::recognizer::check_cancellation(cancellation)?;
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        if format!("{:x}", digest.finalize()) != artifact.sha256 {
            return Err(AppError::Config(
                "Offline model artifact SHA256 mismatch".into(),
            ));
        }
        guards.push(file);
    }
    Ok(guards)
}
