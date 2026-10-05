use crate::{manifest::MAX_INSTALLER_BYTES, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) struct InstallerSignature {
    public_key: PublicKey,
    signature: Signature,
    prehashed: bool,
}

/// Verify an existing installer without downloading, launching or installing it.
pub fn verify_installer(
    path: &Path,
    signature: &str,
    public_key: &str,
    version: &str,
) -> Result<()> {
    InstallerSignature::parse(signature, public_key, version)?.verify(path, &AtomicBool::new(false))
}

fn decode(value: &str, limit: usize) -> Result<String> {
    if value.is_empty() || value.len() > limit {
        return Err(Error::Signature);
    }
    let bytes = STANDARD
        .decode(value.trim())
        .map_err(|_| Error::Signature)?;
    if STANDARD.encode(&bytes) != value.trim() {
        return Err(Error::Signature);
    }
    String::from_utf8(bytes).map_err(|_| Error::Signature)
}

fn public_key(value: &str) -> Result<PublicKey> {
    let text = decode(value, 2048)?;
    let lines: Vec<_> = text.trim_end().lines().collect();
    if lines.len() != 2 || !lines[0].starts_with("untrusted comment: ") {
        return Err(Error::Signature);
    }
    PublicKey::decode(&text).map_err(|_| Error::Signature)
}

impl InstallerSignature {
    pub(crate) fn validate_key(value: &str) -> Result<()> {
        public_key(value)
            .map(|_| ())
            .map_err(|_| Error::Configuration)
    }

    pub(crate) fn parse(value: &str, key: &str, version: &str) -> Result<Self> {
        let text = decode(value, 8192)?;
        let lines: Vec<_> = text.trim_end().lines().collect();
        if lines.len() != 4
            || !lines[0].starts_with("untrusted comment: ")
            || !lines[2].starts_with("trusted comment: ")
        {
            return Err(Error::Signature);
        }
        let record = STANDARD.decode(lines[1]).map_err(|_| Error::Signature)?;
        let global = STANDARD.decode(lines[3]).map_err(|_| Error::Signature)?;
        if record.len() != 74 || global.len() != 64 || !matches!(&record[..2], b"Ed" | b"ED") {
            return Err(Error::Signature);
        }
        let versions: Vec<_> = lines[2]["trusted comment: ".len()..]
            .split('\t')
            .filter_map(|field| field.strip_prefix("version:"))
            .collect();
        if versions != [version] {
            return Err(Error::Version);
        }
        let signature = Signature::decode(&text).map_err(|_| Error::Signature)?;
        Ok(Self {
            public_key: public_key(key)?,
            signature,
            prehashed: &record[..2] == b"ED",
        })
    }

    pub(crate) fn verify(&self, path: &Path, cancel: &AtomicBool) -> Result<()> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        if length == 0 || length > MAX_INSTALLER_BYTES {
            return Err(Error::Size);
        }
        let mut buffer = [0u8; 64 * 1024];
        let mut count = 0u64;
        if self.prehashed {
            let mut verifier = self
                .public_key
                .verify_stream(&self.signature)
                .map_err(|_| Error::Signature)?;
            loop {
                crate::check_cancel(cancel)?;
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                count += read as u64;
                if count > MAX_INSTALLER_BYTES {
                    return Err(Error::Size);
                }
                verifier.update(&buffer[..read]);
            }
            verifier.finalize().map_err(|_| Error::Signature)?;
        } else {
            // Older Ed signatures sign the complete message, so Minisign
            // requires a bounded in-memory buffer for this compatibility mode.
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length as usize)
                .map_err(|_| Error::Size)?;
            loop {
                crate::check_cancel(cancel)?;
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                count += read as u64;
                if count > MAX_INSTALLER_BYTES {
                    return Err(Error::Size);
                }
                bytes.extend_from_slice(&buffer[..read]);
            }
            self.public_key
                .verify(&bytes, &self.signature, true)
                .map_err(|_| Error::Signature)?;
        }
        if count != length || cancel.load(Ordering::Acquire) {
            crate::check_cancel(cancel)?;
            return Err(Error::Size);
        }
        Ok(())
    }
}
