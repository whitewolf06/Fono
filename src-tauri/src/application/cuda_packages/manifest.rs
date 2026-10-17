use super::{invalid, PackageError, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(super) const MAX_MANIFEST: u64 = 64 * 1024;
pub(super) const MAX_SIGNATURE: u64 = 8192;
pub(super) const MAX_ARCHIVE: u64 = 1024 * 1024 * 1024;
pub(super) const MAX_EXTRACTED: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const WORKER: &str = "fono-stt-cuda-worker.exe";
pub(super) const PAYLOAD_NAMES: &[&str] = &[
    WORKER,
    "cublas64_13.dll",
    "cublasLt64_13.dll",
    "cudart64_13.dll",
    "THIRD-PARTY-NOTICES.txt",
    "NVIDIA-CUDA-EULA.txt",
    "NVIDIA-CUDA-third-party.txt",
    "FONO-LICENSE.txt",
    "WHISPER-CPP-LICENSE.txt",
    "DEPENDENCY-NOTICES.txt",
];
pub(super) const NOTICES: &[&str] = &[
    "THIRD-PARTY-NOTICES.txt",
    "NVIDIA-CUDA-EULA.txt",
    "NVIDIA-CUDA-third-party.txt",
    "FONO-LICENSE.txt",
    "WHISPER-CPP-LICENSE.txt",
    "DEPENDENCY-NOTICES.txt",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Archive {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackageFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub purpose: String,
    pub schema_version: u32,
    pub app_version: String,
    pub protocol_version: u16,
    pub platform: String,
    pub revision: u64,
    pub archive: Archive,
    pub files: Vec<PackageFile>,
    pub notices: Vec<String>,
}

pub(super) struct Trust {
    pub key: String,
    pub app_version: String,
    pub protocol_version: u16,
}

impl Trust {
    pub fn production() -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Channel {
            public_key: String,
            endpoint: String,
        }
        let channel: Channel = serde_json::from_str(include_str!("../../../update-channel.json"))
            .map_err(|_| PackageError::Configuration)?;
        if channel.endpoint
            != "https://github.com/whitewolf06/fono/releases/latest/download/latest.json"
        {
            return Err(PackageError::Configuration);
        }
        Ok(Self {
            key: channel.public_key,
            app_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: fono_stt_protocol::PROTOCOL_VERSION,
        })
    }
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn decode(value: &str, limit: usize) -> Result<String> {
    if value.is_empty() || value.len() > limit {
        return Err(PackageError::Signature);
    }
    let bytes = STANDARD
        .decode(value.trim())
        .map_err(|_| PackageError::Signature)?;
    if STANDARD.encode(&bytes) != value.trim() {
        return Err(PackageError::Signature);
    }
    String::from_utf8(bytes).map_err(|_| PackageError::Signature)
}

pub(super) fn verify(bytes: &[u8], signature: &str, trust: &Trust) -> Result<Manifest> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_MANIFEST {
        return Err(PackageError::Manifest);
    }
    let key_text = decode(&trust.key, 2048)?;
    let key_lines: Vec<_> = key_text.trim_end().lines().collect();
    if key_lines.len() != 2 || !key_lines[0].starts_with("untrusted comment: ") {
        return Err(PackageError::Signature);
    }
    let key = PublicKey::decode(&key_text).map_err(|_| PackageError::Signature)?;
    let text = decode(signature, MAX_SIGNATURE as usize)?;
    let lines: Vec<_> = text.trim_end().lines().collect();
    if lines.len() != 4 || !lines[0].starts_with("untrusted comment: ") {
        return Err(PackageError::Signature);
    }
    let signature = Signature::decode(&text).map_err(|_| PackageError::Signature)?;
    // The global signature authenticates this comment as well as the payload.
    // Preserve the updater's signed-version contract and reject duplicates.
    let versions: Vec<_> = signature
        .trusted_comment()
        .split('\t')
        .filter_map(|item| item.strip_prefix("version:"))
        .collect();
    if versions != [trust.app_version.as_str()] {
        return Err(PackageError::Compatibility);
    }
    key.verify(bytes, &signature, true)
        .map_err(|_| PackageError::Signature)?;
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| PackageError::Manifest)?;
    validate(&manifest, trust)?;
    Ok(manifest)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn valid_name(name: &str) -> bool {
    PAYLOAD_NAMES.contains(&name)
}

pub(super) fn validate(manifest: &Manifest, trust: &Trust) -> Result<()> {
    if manifest.purpose != "fono-cuda"
        || manifest.schema_version != 1
        || manifest.platform != "windows-x86_64"
        || manifest.app_version != trust.app_version
        || manifest.protocol_version != trust.protocol_version
    {
        return Err(PackageError::Compatibility);
    }
    if manifest.revision == 0
        || manifest.revision > 9_007_199_254_740_991
        || manifest.archive.name
            != format!("fono-cuda-{}-windows-x86_64.tar.bz2", trust.app_version)
        || manifest.archive.size == 0
        || manifest.archive.size > MAX_ARCHIVE
        || !valid_hash(&manifest.archive.sha256)
        || manifest.files.len() != PAYLOAD_NAMES.len()
        || manifest.notices != NOTICES
    {
        return Err(PackageError::Manifest);
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for file in &manifest.files {
        if !valid_name(&file.path)
            || !names.insert(file.path.as_str())
            || file.size == 0
            || file.size > MAX_ARCHIVE
            || !valid_hash(&file.sha256)
        {
            return Err(PackageError::Manifest);
        }
        total = total.checked_add(file.size).ok_or_else(invalid)?;
    }
    if total > MAX_EXTRACTED {
        return Err(PackageError::Manifest);
    }
    Ok(())
}
