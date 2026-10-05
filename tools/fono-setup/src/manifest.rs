use crate::{signature::InstallerSignature, Config, Error, Result};
use reqwest::Url;
use serde::Deserialize;
use std::collections::BTreeMap;

pub(crate) const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
pub const MINIMUM_SUPPORTED_VERSION: &str = "0.6.12";

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Debug, Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

pub(crate) struct Release {
    pub version: String,
    pub url: Url,
    pub signature: InstallerSignature,
    pub signature_text: String,
}

pub(crate) fn parse(bytes: &[u8], config: &Config) -> Result<Release> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Error::Manifest);
    }
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| Error::Manifest)?;
    let version = semver::Version::parse(&manifest.version).map_err(|_| Error::Manifest)?;
    if !version.pre.is_empty()
        || !version.build.is_empty()
        || version.to_string() != manifest.version
    {
        return Err(Error::Manifest);
    }
    if version < semver::Version::parse(MINIMUM_SUPPORTED_VERSION).expect("valid minimum version") {
        return Err(Error::Version);
    }
    if manifest.platforms.len() != 1 {
        return Err(Error::Manifest);
    }
    let platform = manifest
        .platforms
        .get("windows-x86_64")
        .ok_or(Error::Manifest)?;
    let url = Url::parse(&platform.url).map_err(|_| Error::UnsafeUrl)?;
    let prefix = format!(
        "/{}/releases/download/v{}/",
        config.repository, manifest.version
    );
    validate_github(&url, &prefix)?;
    let filename = url.path().strip_prefix(&prefix).ok_or(Error::UnsafeUrl)?;
    if filename != format!("Fono_{}_x64-setup.exe", manifest.version) {
        return Err(Error::UnsafeUrl);
    }
    let signature =
        InstallerSignature::parse(&platform.signature, &config.public_key, &manifest.version)?;
    Ok(Release {
        version: manifest.version,
        url,
        signature,
        signature_text: platform.signature.clone(),
    })
}

pub(crate) fn validate_config(config: &Config) -> Result<()> {
    // Restrict the trust boundary to this project's public channel. The build
    // embeds its pinned public key; downloaded documents never select a key.
    if config.repository != "whitewolf06/fono" {
        return Err(Error::Configuration);
    }
    let endpoint = Url::parse(&config.endpoint).map_err(|_| Error::Configuration)?;
    let path = format!(
        "/{}/releases/latest/download/latest.json",
        config.repository
    );
    validate_github(&endpoint, &path).map_err(|_| Error::Configuration)?;
    if endpoint.path() != path {
        return Err(Error::Configuration);
    }
    InstallerSignature::validate_key(&config.public_key)
}

pub(crate) fn validate_github(url: &Url, prefix: &str) -> Result<()> {
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with(prefix)
        || url.path().contains('%')
    {
        return Err(Error::UnsafeUrl);
    }
    Ok(())
}

pub(crate) fn validate_redirect(url: &Url, repository: &str) -> Result<()> {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::UnsafeUrl);
    }
    match url.host_str() {
        Some("github.com") => validate_github(url, &format!("/{repository}/releases/")),
        // GitHub's signed, short-lived asset redirect is followed only within
        // the current request and is never persisted for resumed downloads.
        Some("release-assets.githubusercontent.com") => Ok(()),
        _ => Err(Error::UnsafeUrl),
    }
}
