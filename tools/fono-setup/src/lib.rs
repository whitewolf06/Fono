mod cache;
pub mod cli;
pub mod config;
#[cfg(test)]
mod core_tests;
mod download;
mod download_response;
#[cfg(test)]
mod download_test_support;
#[cfg(test)]
mod download_tests;
mod error;
mod manifest;
mod signature;

pub use error::{Error, Result};
pub use signature::verify_installer;

use cache::{Cache, CacheLock};
use reqwest::blocking::Client;
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: String,
    pub public_key: String,
    pub repository: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Resolving,
    Downloading,
    Verifying,
    Ready,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
}

pub struct VerifiedInstaller {
    pub path: PathBuf,
    pub version: String,
    // Only the verifier constructs this token. Keep the cache lock and a
    // Windows read-only sharing handle until the installer has finished.
    _cache_lock: CacheLock,
    _protected_file: File,
    _directory_handle: File,
}

pub struct Bootstrapper {
    config: Config,
    cache_dir: PathBuf,
    client: Client,
}

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

impl Bootstrapper {
    pub fn new(config: Config, cache_dir: PathBuf) -> Result<Self> {
        manifest::validate_config(&config)?;
        if !cache_dir.is_absolute()
            || cache_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(Error::Configuration);
        }
        #[cfg(windows)]
        match cache_dir.components().next() {
            Some(std::path::Component::Prefix(prefix))
                if matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) => {}
            _ => return Err(Error::Configuration),
        }
        let client = download::client()?;
        Ok(Self {
            config,
            cache_dir,
            client,
        })
    }

    pub fn resolve_manifest(&self, cancel: &AtomicBool) -> Result<ReleaseInfo> {
        let release = download::resolve(&self.client, &self.config, cancel)?;
        Ok(ReleaseInfo {
            version: release.version,
            url: release.url.to_string(),
        })
    }

    pub fn run(
        &self,
        cancel: Arc<AtomicBool>,
        progress: impl Fn(Progress),
    ) -> Result<VerifiedInstaller> {
        check_cancel(&cancel)?;
        let cache_lock = CacheLock::acquire(&self.cache_dir)?;
        progress(Progress {
            phase: Phase::Resolving,
            downloaded_bytes: 0,
            total_bytes: None,
            version: None,
        });
        let release = download::resolve(&self.client, &self.config, &cancel)?;
        let cache = Cache::for_release(&self.cache_dir, &release)?;
        if cache.installer.is_file() {
            let file = cache::protected_read(&cache.installer)?;
            let size = file.metadata()?.len();
            progress(Progress {
                phase: Phase::Verifying,
                downloaded_bytes: size,
                total_bytes: Some(size),
                version: Some(release.version.clone()),
            });
            match release.signature.verify(&cache.installer, &cancel) {
                Ok(()) => return Self::ready(cache, release.version, file, cache_lock, &progress),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(_) => {
                    drop(file);
                    std::fs::remove_file(&cache.installer)?;
                }
            }
        }
        download::installer(
            &self.client,
            &self.config,
            &release,
            &cache,
            &cancel,
            &progress,
        )?;
        check_cancel(&cancel)?;
        let size = cache.partial.metadata()?.len();
        progress(Progress {
            phase: Phase::Verifying,
            downloaded_bytes: size,
            total_bytes: Some(size),
            version: Some(release.version.clone()),
        });
        if let Err(error) = release.signature.verify(&cache.partial, &cancel) {
            if !matches!(error, Error::Cancelled) {
                cache.reset_partial()?;
            }
            return Err(error);
        }
        check_cancel(&cancel)?;
        cache.promote()?;
        let file = cache::protected_read(&cache.installer)?;
        // Verify after promotion with the immutable executable handle held.
        release.signature.verify(&cache.installer, &cancel)?;
        Self::ready(cache, release.version, file, cache_lock, &progress)
    }

    fn ready(
        cache: Cache,
        version: String,
        file: File,
        lock: CacheLock,
        progress: &impl Fn(Progress),
    ) -> Result<VerifiedInstaller> {
        let size = file.metadata()?.len();
        progress(Progress {
            phase: Phase::Ready,
            downloaded_bytes: size,
            total_bytes: Some(size),
            version: Some(version.clone()),
        });
        Ok(VerifiedInstaller {
            path: cache.installer,
            version,
            _cache_lock: lock,
            _protected_file: file,
            _directory_handle: cache.directory_handle,
        })
    }
}
