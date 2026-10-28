//! Resumable, verified model acquisition for the standalone server.
//!
//! Desktop downloads keep their existing lifecycle. Server downloads retain
//! only pinned partial artifacts in a private staging directory so the next
//! invocation can continue after a network interruption.

use super::download::{emit_download, DownloadPhase, DownloadProgress};
use crate::error::{SttError as AppError, SttResult as AppResult};
use crate::offline_models::{OfflineArtifact, OfflineModelSpec};
use fono_types::WhisperArtifact;
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const BUFFER_BYTES: usize = 64 * 1024;
const PROGRESS_BYTES: u64 = 1024 * 1024;
const NOTICE_NAME: &str = "FONO-MODEL-NOTICE.md";
const NOTICE: &str = include_str!("../../../../docs/evidence/licenses/offline-stt-NOTICE.md");
const STAGING_NAME: &str = ".fono-server-downloads";

struct HttpSource {
    client: Client,
    allow_local_http: bool,
}

impl HttpSource {
    fn pinned_https() -> AppResult<Self> {
        let client = Client::builder()
            .https_only(true)
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|_| AppError::Config("Cannot initialize model download client".into()))?;
        Ok(Self {
            client,
            allow_local_http: false,
        })
    }

    #[cfg(test)]
    fn local_test() -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            allow_local_http: true,
        }
    }

    fn fetch(&self, url: &str, offset: u64) -> AppResult<Response> {
        let parsed = reqwest::Url::parse(url)
            .map_err(|_| AppError::Config("Invalid pinned model URL".into()))?;
        if (parsed.scheme() != "https"
            && !(self.allow_local_http
                && parsed.scheme() == "http"
                && parsed.host_str().is_some_and(|host| host == "127.0.0.1")))
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(AppError::Config("Invalid pinned model URL".into()));
        }
        let mut request = self.client.get(parsed).header(ACCEPT_ENCODING, "identity");
        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }
        request
            .send()
            .map_err(|_| AppError::Config("Model download request failed".into()))
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn check_artifact(filename: &str, bytes: u64, sha256: &str) -> AppResult<()> {
    if !safe_component(filename)
        || bytes == 0
        || sha256.len() != 64
        || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::Config("Invalid pinned model artifact".into()));
    }
    Ok(())
}

fn check_offline_spec(spec: &OfflineModelSpec) -> AppResult<u64> {
    if !safe_component(&spec.directory) || spec.files.is_empty() {
        return Err(AppError::Config(
            "Invalid pinned offline model package".into(),
        ));
    }
    let mut names = HashSet::new();
    let mut total = 0_u64;
    for artifact in &spec.files {
        check_artifact(&artifact.filename, artifact.bytes, &artifact.sha256)?;
        if artifact.filename == NOTICE_NAME
            || artifact.filename.ends_with(".part")
            || !names.insert(&artifact.filename)
        {
            return Err(AppError::Config("Duplicate model artifact filename".into()));
        }
        total = total
            .checked_add(artifact.bytes)
            .ok_or_else(|| AppError::Config("Model package size overflow".into()))?;
    }
    Ok(total)
}

fn metadata_if_present(path: &Path) -> AppResult<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn regular_file(path: &Path) -> AppResult<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Config(
            "Model artifact must be a regular file".into(),
        ));
    }
    Ok(metadata)
}

fn ordinary_directory(path: &Path) -> AppResult<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::Config(
            "Model directory must be a real directory".into(),
        ));
    }
    Ok(())
}

fn checked_models_dir(models_dir: &Path, create: bool) -> AppResult<bool> {
    // Create and inspect one ancestor at a time. create_dir_all could follow a
    // symlinked parent before we get a chance to reject it.
    for ancestor in models_dir.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if metadata_if_present(ancestor)?.is_none() {
            if !create {
                return Ok(false);
            }
            match std::fs::create_dir(ancestor) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        ordinary_directory(ancestor)?;
    }
    Ok(true)
}

fn staging_dir(models_dir: &Path) -> AppResult<PathBuf> {
    let path = models_dir.join(STAGING_NAME);
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    ordinary_directory(&path)?;
    Ok(path)
}

fn lock_model(staging: &Path, name: &str) -> AppResult<File> {
    let path = staging.join(format!("{name}.lock"));
    if let Some(metadata) = metadata_if_present(&path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Config(
                "Model download lock is not a regular file".into(),
            ));
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.try_lock()
        .map_err(|_| AppError::Busy("model download is already active".into()))?;
    Ok(file)
}

fn sha256_file(path: &Path, expected_bytes: u64) -> AppResult<String> {
    let metadata = regular_file(path)?;
    if metadata.len() != expected_bytes {
        return Err(AppError::Config("Model artifact size mismatch".into()));
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_file(path: &Path, bytes: u64, sha256: &str) -> AppResult<()> {
    if sha256_file(path, bytes)? != sha256 {
        return Err(AppError::Config("Model artifact SHA256 mismatch".into()));
    }
    Ok(())
}

fn verify_notice(path: &Path) -> AppResult<()> {
    if regular_file(path)?.len() != NOTICE.len() as u64 {
        return Err(AppError::Config(
            "Model package NOTICE size mismatch".into(),
        ));
    }
    if std::fs::read(path)? != NOTICE.as_bytes() {
        return Err(AppError::Config("Model package NOTICE mismatch".into()));
    }
    Ok(())
}

fn publish_file(partial: &Path, target: &Path, repairing: bool) -> AppResult<()> {
    if metadata_if_present(target)?.is_some() {
        if !repairing {
            return Err(AppError::Config(
                "Model target appeared during download".into(),
            ));
        }
        regular_file(target)?;
    } else if repairing {
        return Err(AppError::Config(
            "Damaged model target disappeared during repair".into(),
        ));
    }
    #[cfg(windows)]
    {
        if repairing {
            return Err(AppError::Config(
                "Atomic model repair requires Ubuntu/Linux".into(),
            ));
        }
        // Windows rename does not replace an existing destination.
        std::fs::rename(partial, target)?;
    }
    #[cfg(not(windows))]
    {
        if repairing {
            // POSIX rename replaces the damaged file in one namespace operation.
            std::fs::rename(partial, target)?;
        } else {
            // Hard-link publication is atomic and refuses an existing destination.
            std::fs::hard_link(partial, target)?;
            std::fs::remove_file(partial)?;
        }
    }
    Ok(())
}

fn content_range_matches(value: Option<&str>, offset: u64, expected: u64) -> bool {
    let Some(value) = value else { return false };
    let Some(remainder) = value.strip_prefix("bytes ") else {
        return false;
    };
    let Some((range, total)) = remainder.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    start.parse::<u64>() == Ok(offset)
        && end.parse::<u64>() == Ok(expected - 1)
        && total.parse::<u64>() == Ok(expected)
}

fn prepare_partial(path: &Path, bytes: u64, sha256: &str) -> AppResult<(File, u64)> {
    if let Some(metadata) = metadata_if_present(path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Config(
                "Model partial must be a regular file".into(),
            ));
        }
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let metadata = regular_file(path)?;
    let mut offset = metadata.len();
    if offset > bytes || (offset == bytes && sha256_file(path, bytes)? != sha256) {
        file.set_len(0)?;
        file.sync_all()?;
        offset = 0;
    }
    file.seek(SeekFrom::Start(offset))?;
    Ok((file, offset))
}

fn download_file(
    source: &HttpSource,
    artifact: &OfflineArtifact,
    partial: &Path,
    target: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(u64),
    verifying: impl FnOnce(),
) -> AppResult<()> {
    check_artifact(&artifact.filename, artifact.bytes, &artifact.sha256)?;
    let repairing = if metadata_if_present(target)?.is_some() {
        regular_file(target)?;
        match verify_file(target, artifact.bytes, &artifact.sha256) {
            Ok(()) => {
                // On Linux the process can die after hard-linking a verified
                // stage target but before unlinking its owned partial name.
                if metadata_if_present(partial)?.is_some() {
                    regular_file(partial)?;
                    std::fs::remove_file(partial)?;
                }
                progress(artifact.bytes);
                return Ok(());
            }
            Err(AppError::Config(_)) => true,
            Err(error) => return Err(error),
        }
    } else {
        false
    };
    let (mut file, mut offset) = prepare_partial(partial, artifact.bytes, &artifact.sha256)?;
    progress(offset);
    if offset < artifact.bytes {
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        let mut response = source.fetch(&artifact.url, offset)?;
        let status = response.status().as_u16();
        if offset > 0 && status == 200 {
            if response
                .content_length()
                .is_some_and(|length| length != artifact.bytes)
            {
                return Err(AppError::Config("Model download length mismatch".into()));
            }
            // Some artifact hosts ignore Range. Never append a full body to a
            // partial body: restart from byte zero using this response.
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            offset = 0;
            progress(0);
        } else if offset > 0 && status == 206 {
            let content_range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok());
            if !content_range_matches(content_range, offset, artifact.bytes) {
                return Err(AppError::Config(
                    "Invalid model download Content-Range".into(),
                ));
            }
        } else if status != 200 {
            return Err(AppError::Config(
                "Unexpected model download response".into(),
            ));
        }
        if let Some(length) = response.content_length() {
            if length != artifact.bytes - offset {
                return Err(AppError::Config("Model download length mismatch".into()));
            }
        }
        let mut buffer = [0_u8; BUFFER_BYTES];
        let mut next_progress = offset.saturating_add(PROGRESS_BYTES);
        loop {
            if cancellation.load(Ordering::Acquire) {
                file.sync_all()?;
                return Err(AppError::Cancelled("model download cancelled".into()));
            }
            let count = match response.read(&mut buffer) {
                Ok(count) => count,
                Err(error) => {
                    file.sync_all()?;
                    return Err(error.into());
                }
            };
            if count == 0 {
                break;
            }
            if offset.saturating_add(count as u64) > artifact.bytes {
                file.set_len(0)?;
                file.sync_all()?;
                return Err(AppError::Config(
                    "Model download exceeds pinned size".into(),
                ));
            }
            file.write_all(&buffer[..count])?;
            offset += count as u64;
            if offset >= next_progress {
                progress(offset);
                next_progress = offset.saturating_add(PROGRESS_BYTES);
            }
        }
        file.sync_all()?;
    }
    if offset != artifact.bytes {
        return Err(AppError::Config(
            "Model download incomplete; partial retained for retry".into(),
        ));
    }
    progress(offset);
    drop(file);
    if cancellation.load(Ordering::Acquire) {
        return Err(AppError::Cancelled("model download cancelled".into()));
    }
    verifying();
    if let Err(error) = verify_file(partial, artifact.bytes, &artifact.sha256) {
        // The partial is exclusively owned by this lock. Discard only this
        // known file so the next pull starts from a trusted byte zero.
        if matches!(error, AppError::Config(_)) {
            let partial_file = OpenOptions::new().write(true).open(partial)?;
            partial_file.set_len(0)?;
            partial_file.sync_all()?;
        }
        return Err(error);
    }
    publish_file(partial, target, repairing)
}

pub fn verify_whisper(models_dir: &Path, artifact: &WhisperArtifact) -> AppResult<bool> {
    check_artifact(&artifact.filename, artifact.bytes, &artifact.sha256)?;
    if !checked_models_dir(models_dir, false)? {
        return Ok(false);
    }
    let target = models_dir.join(&artifact.filename);
    if metadata_if_present(&target)?.is_none() {
        return Ok(false);
    }
    verify_file(&target, artifact.bytes, &artifact.sha256)?;
    Ok(true)
}

pub fn verify_offline(models_dir: &Path, spec: &OfflineModelSpec) -> AppResult<bool> {
    check_offline_spec(spec)?;
    if !checked_models_dir(models_dir, false)? {
        return Ok(false);
    }
    let target = models_dir.join(&spec.directory);
    if metadata_if_present(&target)?.is_none() {
        return Ok(false);
    }
    ordinary_directory(&target)?;
    check_package_contents(&target, spec)?;
    for artifact in &spec.files {
        verify_file(
            &target.join(&artifact.filename),
            artifact.bytes,
            &artifact.sha256,
        )?;
    }
    verify_notice(&target.join(NOTICE_NAME))?;
    Ok(true)
}

fn check_package_contents(directory: &Path, spec: &OfflineModelSpec) -> AppResult<()> {
    let expected: HashSet<&str> = spec
        .files
        .iter()
        .map(|artifact| artifact.filename.as_str())
        .chain(std::iter::once(NOTICE_NAME))
        .collect();
    for child in std::fs::read_dir(directory)? {
        let child = child?;
        let filename = child.file_name();
        if !filename
            .to_str()
            .is_some_and(|name| expected.contains(name))
        {
            return Err(AppError::Config(
                "Model package contains unknown files".into(),
            ));
        }
        regular_file(&child.path())?;
    }
    Ok(())
}

fn offline_target_healthy(directory: &Path, spec: &OfflineModelSpec) -> AppResult<bool> {
    ordinary_directory(directory)?;
    check_package_contents(directory, spec)?;
    for artifact in &spec.files {
        let path = directory.join(&artifact.filename);
        if metadata_if_present(&path)?.is_none() {
            return Ok(false);
        }
        regular_file(&path)?;
        match verify_file(&path, artifact.bytes, &artifact.sha256) {
            Ok(()) => {}
            Err(AppError::Config(_)) => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    let notice = directory.join(NOTICE_NAME);
    if metadata_if_present(&notice)?.is_none() {
        return Ok(false);
    }
    match verify_notice(&notice) {
        Ok(()) => Ok(true),
        Err(AppError::Config(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn linux_rename(first: &Path, second: &Path, flags: u32) -> AppResult<()> {
    use std::ffi::CString;
    use std::os::raw::c_char;
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const c_char,
            newdirfd: i32,
            newpath: *const c_char,
            flags: u32,
        ) -> i32;
    }

    let first = CString::new(first.as_os_str().as_bytes())
        .map_err(|_| AppError::Config("Invalid model package path".into()))?;
    let second = CString::new(second.as_os_str().as_bytes())
        .map_err(|_| AppError::Config("Invalid model package path".into()))?;
    const AT_FDCWD: i32 = -100;
    // SAFETY: both paths are valid, NUL-free C strings held alive for this call.
    let status = unsafe { renameat2(AT_FDCWD, first.as_ptr(), AT_FDCWD, second.as_ptr(), flags) };
    if status != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn exchange_directories(first: &Path, second: &Path) -> AppResult<()> {
    // RENAME_EXCHANGE swaps both existing entries atomically.
    linux_rename(first, second, 2)
}

fn publish_offline_directory(partial: &Path, target: &Path) -> AppResult<()> {
    #[cfg(target_os = "linux")]
    {
        // RENAME_NOREPLACE atomically refuses a newly appeared target.
        linux_rename(partial, target, 1)
    }
    #[cfg(not(target_os = "linux"))]
    {
        if metadata_if_present(target)?.is_some() {
            return Err(AppError::Config(
                "Model target appeared during download".into(),
            ));
        }
        std::fs::rename(partial, target)?;
        Ok(())
    }
}

fn remove_known_package(directory: &Path, spec: &OfflineModelSpec) -> AppResult<()> {
    ordinary_directory(directory)?;
    check_package_contents(directory, spec)?;
    for artifact in &spec.files {
        let path = directory.join(&artifact.filename);
        if metadata_if_present(&path)?.is_some() {
            regular_file(&path)?;
            std::fs::remove_file(path)?;
        }
    }
    let notice = directory.join(NOTICE_NAME);
    if metadata_if_present(&notice)?.is_some() {
        regular_file(&notice)?;
        std::fs::remove_file(notice)?;
    }
    std::fs::remove_dir(directory)?;
    Ok(())
}

fn discard_or_preserve_stage(
    staging: &Path,
    stage: &Path,
    spec: &OfflineModelSpec,
) -> AppResult<()> {
    let Some(metadata) = metadata_if_present(stage)? else {
        return Ok(());
    };
    if !metadata.file_type().is_symlink() && metadata.is_dir() {
        let known: HashSet<String> = spec
            .files
            .iter()
            .flat_map(|artifact| {
                [
                    artifact.filename.clone(),
                    format!("{}.part", artifact.filename),
                ]
            })
            .chain(std::iter::once(NOTICE_NAME.to_owned()))
            .collect();
        let mut children = Vec::new();
        let mut safe = true;
        for child in std::fs::read_dir(stage)? {
            let child = child?;
            let filename = child.file_name();
            let metadata = std::fs::symlink_metadata(child.path())?;
            if !filename.to_str().is_some_and(|name| known.contains(name))
                || metadata.file_type().is_symlink()
                || !metadata.is_file()
            {
                safe = false;
                break;
            }
            children.push(child.path());
        }
        if safe {
            for child in children {
                regular_file(&child)?;
                std::fs::remove_file(child)?;
            }
            std::fs::remove_dir(stage)?;
            return Ok(());
        }
    }
    let preserved = staging.join(format!(
        "{}-preserved-{}",
        spec.directory,
        uuid::Uuid::new_v4()
    ));
    std::fs::rename(stage, preserved)?;
    Ok(())
}

fn pull_whisper_with_source(
    models_dir: &Path,
    artifact: &WhisperArtifact,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
    source: &HttpSource,
) -> AppResult<()> {
    check_artifact(&artifact.filename, artifact.bytes, &artifact.sha256)?;
    checked_models_dir(models_dir, true)?;
    let staging = staging_dir(models_dir)?;
    let _lock = lock_model(&staging, &artifact.filename)?;
    let id = format!("whisper:{}", artifact.variant);
    let target = models_dir.join(&artifact.filename);
    let partial = staging.join(format!("{}.part", artifact.filename));
    emit_download(
        progress,
        &id,
        &artifact.filename,
        DownloadPhase::Started,
        0,
        Some(artifact.bytes),
    );
    let entry = OfflineArtifact {
        filename: artifact.filename.clone(),
        url: artifact.url.clone(),
        sha256: artifact.sha256.clone(),
        bytes: artifact.bytes,
    };
    let result = download_file(
        source,
        &entry,
        &partial,
        &target,
        cancellation,
        |bytes| {
            emit_download(
                progress,
                &id,
                &artifact.filename,
                DownloadPhase::Downloading,
                bytes,
                Some(artifact.bytes),
            );
        },
        || {
            emit_download(
                progress,
                &id,
                &artifact.filename,
                DownloadPhase::Verifying,
                artifact.bytes,
                Some(artifact.bytes),
            );
        },
    );
    match &result {
        Ok(()) => emit_download(
            progress,
            &id,
            &artifact.filename,
            DownloadPhase::Completed,
            artifact.bytes,
            Some(artifact.bytes),
        ),
        Err(AppError::Cancelled(_)) => emit_download(
            progress,
            &id,
            &artifact.filename,
            DownloadPhase::Cancelled,
            0,
            Some(artifact.bytes),
        ),
        Err(_) => emit_download(
            progress,
            &id,
            &artifact.filename,
            DownloadPhase::Failed,
            0,
            Some(artifact.bytes),
        ),
    }
    result
}

pub fn pull_whisper(
    models_dir: &Path,
    artifact: &WhisperArtifact,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
) -> AppResult<()> {
    let pinned = WhisperArtifact::all()
        .map_err(|_| AppError::Config("Invalid embedded Whisper catalog".into()))?
        .into_iter()
        .any(|entry| {
            entry.variant == artifact.variant
                && entry.filename == artifact.filename
                && entry.bytes == artifact.bytes
                && entry.sha256 == artifact.sha256
                && entry.url == artifact.url
        });
    if !pinned {
        return Err(AppError::Config("Whisper artifact is not pinned".into()));
    }
    let source = HttpSource::pinned_https()?;
    pull_whisper_with_source(models_dir, artifact, cancellation, progress, &source)
}

fn pull_offline_with_source(
    models_dir: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
    source: &HttpSource,
) -> AppResult<()> {
    let total = check_offline_spec(spec)?;
    checked_models_dir(models_dir, true)?;
    let staging = staging_dir(models_dir)?;
    let _lock = lock_model(&staging, &spec.directory)?;
    let id = spec.id.public_id();
    let target = models_dir.join(&spec.directory);
    let partial = staging.join(format!("{}.part", spec.directory));
    let repairing = if metadata_if_present(&target)?.is_some() {
        ordinary_directory(&target)?;
        let healthy = match offline_target_healthy(&target, spec) {
            Ok(healthy) => healthy,
            // Unknown children or symlinks are never followed or deleted.
            // Linux exchange can preserve the complete old directory.
            Err(AppError::Config(_)) => false,
            Err(error) => return Err(error),
        };
        if healthy {
            discard_or_preserve_stage(&staging, &partial, spec)?;
            emit_download(
                progress,
                &id,
                &spec.directory,
                DownloadPhase::Completed,
                total,
                Some(total),
            );
            return Ok(());
        }
        true
    } else {
        false
    };
    match std::fs::create_dir(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    ordinary_directory(&partial)?;
    emit_download(
        progress,
        &id,
        &spec.directory,
        DownloadPhase::Started,
        0,
        Some(total),
    );
    let result = (|| {
        let mut completed = 0_u64;
        for artifact in &spec.files {
            if cancellation.load(Ordering::Acquire) {
                return Err(AppError::Cancelled("model download cancelled".into()));
            }
            let stage_target = partial.join(&artifact.filename);
            let stage_partial = partial.join(format!("{}.part", artifact.filename));
            download_file(
                source,
                artifact,
                &stage_partial,
                &stage_target,
                cancellation,
                |bytes| {
                    emit_download(
                        progress,
                        &id,
                        &spec.directory,
                        DownloadPhase::Downloading,
                        completed + bytes,
                        Some(total),
                    );
                },
                || {},
            )?;
            completed += artifact.bytes;
        }
        emit_download(
            progress,
            &id,
            &spec.directory,
            DownloadPhase::Verifying,
            total,
            Some(total),
        );
        for artifact in &spec.files {
            verify_file(
                &partial.join(&artifact.filename),
                artifact.bytes,
                &artifact.sha256,
            )?;
        }
        let notice = partial.join(NOTICE_NAME);
        if metadata_if_present(&notice)?.is_none() {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&notice)?;
            file.write_all(NOTICE.as_bytes())?;
            file.sync_all()?;
        } else {
            regular_file(&notice)?;
            if matches!(verify_notice(&notice), Err(AppError::Config(_))) {
                // Only this pinned NOTICE is rewritten inside an unpublished
                // package. A crash during writing is repaired on retry.
                let mut file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&notice)?;
                file.write_all(NOTICE.as_bytes())?;
                file.sync_all()?;
            } else {
                verify_notice(&notice)?;
            }
        }
        check_package_contents(&partial, spec)?;
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        if repairing {
            ordinary_directory(&target)?;
            #[cfg(target_os = "linux")]
            {
                exchange_directories(&partial, &target)?;
                // The previous damaged package is now in our private stage.
                discard_or_preserve_stage(&staging, &partial, spec)?;
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err(AppError::Config(
                    "Atomic offline model repair requires Ubuntu/Linux".into(),
                ));
            }
        } else {
            if metadata_if_present(&target)?.is_some() {
                return Err(AppError::Config(
                    "Model target appeared during download".into(),
                ));
            }
            publish_offline_directory(&partial, &target)?;
        }
        Ok(())
    })();
    match &result {
        Ok(()) => emit_download(
            progress,
            &id,
            &spec.directory,
            DownloadPhase::Completed,
            total,
            Some(total),
        ),
        Err(AppError::Cancelled(_)) => emit_download(
            progress,
            &id,
            &spec.directory,
            DownloadPhase::Cancelled,
            0,
            Some(total),
        ),
        Err(_) => emit_download(
            progress,
            &id,
            &spec.directory,
            DownloadPhase::Failed,
            0,
            Some(total),
        ),
    }
    result
}

pub fn pull_offline(
    models_dir: &Path,
    spec: &OfflineModelSpec,
    cancellation: &AtomicBool,
    progress: &dyn DownloadProgress,
) -> AppResult<()> {
    let supplied = serde_json::to_value(spec)?;
    let pinned = crate::offline_models::specifications()?
        .iter()
        .any(|entry| serde_json::to_value(entry).is_ok_and(|value| value == supplied));
    if !pinned {
        return Err(AppError::Config(
            "Offline model package is not pinned".into(),
        ));
    }
    let source = HttpSource::pinned_https()?;
    pull_offline_with_source(models_dir, spec, cancellation, progress, &source)
}

pub fn remove_whisper(models_dir: &Path, artifact: &WhisperArtifact) -> AppResult<()> {
    check_artifact(&artifact.filename, artifact.bytes, &artifact.sha256)?;
    checked_models_dir(models_dir, false)?;
    let staging = staging_dir(models_dir)?;
    let _lock = lock_model(&staging, &artifact.filename)?;
    let target = models_dir.join(&artifact.filename);
    regular_file(&target)?;
    verify_file(&target, artifact.bytes, &artifact.sha256)?;
    std::fs::remove_file(target)?;
    Ok(())
}

pub fn remove_offline(models_dir: &Path, spec: &OfflineModelSpec) -> AppResult<()> {
    check_offline_spec(spec)?;
    checked_models_dir(models_dir, false)?;
    let staging = staging_dir(models_dir)?;
    let _lock = lock_model(&staging, &spec.directory)?;
    let target = models_dir.join(&spec.directory);
    ordinary_directory(&target)?;
    // Explicit remove requires a complete, verified package. A damaged
    // default/preloaded package is restored by pull without exposing a delete.
    verify_offline(models_dir, spec)?;
    remove_known_package(&target, spec)
}

#[cfg(test)]
mod tests {
    use super::super::download::DownloadEvent;
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    struct FixtureDir(PathBuf);

    impl FixtureDir {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("fono-server-store-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn models(&self) -> PathBuf {
            self.0.join("models")
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            // The only recursive removal in these tests is a unique fixture
            // directory confirmed to remain under the system temp root.
            if let (Ok(root), Ok(temp)) =
                (self.0.canonicalize(), std::env::temp_dir().canonicalize())
            {
                if root.starts_with(temp)
                    && root.file_name().is_some_and(|name| {
                        name.to_string_lossy()
                            .starts_with("fono-server-store-test-")
                    })
                {
                    std::fs::remove_dir_all(root).unwrap();
                }
            }
        }
    }

    struct Reply {
        path: &'static str,
        range: Option<&'static str>,
        status: u16,
        content_range: Option<&'static str>,
        declared_length: u64,
        body: &'static [u8],
    }

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0 && request.len() < 16 * 1024);
            request.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(request).unwrap()
    }

    fn serve(replies: Vec<Reply>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing fixture HTTP request");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fixture accept failed: {error}"),
                    }
                };
                let request = read_request(&mut stream);
                assert!(request.starts_with(&format!("GET {} HTTP/1.1\r\n", reply.path)));
                let actual_range = request.lines().find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("range: ")
                        .map(str::to_owned)
                });
                assert_eq!(actual_range.as_deref(), reply.range);
                let reason = if reply.status == 206 {
                    "Partial Content"
                } else {
                    "OK"
                };
                let range = reply
                    .content_range
                    .map_or_else(String::new, |value| format!("Content-Range: {value}\r\n"));
                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                    reply.status, reason, reply.declared_length, range
                );
                stream.write_all(response.as_bytes()).unwrap();
                // A rejected Content-Range can close the client before it
                // reads the fixture body; the header is what this test needs.
                let _ = stream.write_all(reply.body);
            }
        });
        (format!("http://{address}"), handle)
    }

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn whisper(filename: &str, url: String, bytes: &[u8]) -> WhisperArtifact {
        WhisperArtifact {
            variant: "fixture".into(),
            filename: filename.into(),
            bytes: bytes.len() as u64,
            sha256: hash(bytes),
            url,
        }
    }

    fn offline(urls: [&str; 2]) -> OfflineModelSpec {
        let mut spec = crate::offline_models::specifications()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        spec.directory = "fixture-offline".into();
        spec.files = vec![
            OfflineArtifact {
                filename: "first.bin".into(),
                url: urls[0].into(),
                bytes: 6,
                sha256: hash(b"first!"),
            },
            OfflineArtifact {
                filename: "second.bin".into(),
                url: urls[1].into(),
                bytes: 6,
                sha256: hash(b"second"),
            },
        ];
        spec
    }

    fn pull_fixture_whisper(models: &Path, artifact: &WhisperArtifact) -> AppResult<()> {
        pull_whisper_with_source(
            models,
            artifact,
            &AtomicBool::new(false),
            &|_: DownloadEvent| {},
            &HttpSource::local_test(),
        )
    }

    fn pull_fixture_offline(models: &Path, spec: &OfflineModelSpec) -> AppResult<()> {
        pull_offline_with_source(
            models,
            spec,
            &AtomicBool::new(false),
            &|_: DownloadEvent| {},
            &HttpSource::local_test(),
        )
    }

    #[test]
    fn whisper_resumes_interrupted_http_body() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        let (base, server) = serve(vec![Reply {
            path: "/whisper",
            range: None,
            status: 200,
            content_range: None,
            declared_length: 6,
            body: b"abc",
        }]);
        let artifact = whisper("fixture.bin", format!("{base}/whisper"), b"abcdef");
        assert!(pull_fixture_whisper(&models, &artifact).is_err());
        server.join().unwrap();
        let partial = models.join(STAGING_NAME).join("fixture.bin.part");
        assert_eq!(std::fs::read(&partial).unwrap(), b"abc");
        assert!(!verify_whisper(&models, &artifact).unwrap());

        let (base, server) = serve(vec![Reply {
            path: "/whisper",
            range: Some("bytes=3-"),
            status: 206,
            content_range: Some("bytes 3-5/6"),
            declared_length: 3,
            body: b"def",
        }]);
        let artifact = whisper("fixture.bin", format!("{base}/whisper"), b"abcdef");
        pull_fixture_whisper(&models, &artifact).unwrap();
        server.join().unwrap();
        assert!(verify_whisper(&models, &artifact).unwrap());
        assert_eq!(
            std::fs::read(models.join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        assert!(!partial.exists());
    }

    #[test]
    fn invalid_range_keeps_existing_partial_unchanged() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        let stage = models.join(STAGING_NAME);
        std::fs::create_dir_all(&stage).unwrap();
        let partial = stage.join("fixture.bin.part");
        std::fs::write(&partial, b"abc").unwrap();
        let (base, server) = serve(vec![Reply {
            path: "/whisper",
            range: Some("bytes=3-"),
            status: 206,
            content_range: Some("bytes 2-5/6"),
            declared_length: 3,
            body: b"def",
        }]);
        let artifact = whisper("fixture.bin", format!("{base}/whisper"), b"abcdef");
        assert!(pull_fixture_whisper(&models, &artifact).is_err());
        server.join().unwrap();
        assert_eq!(std::fs::read(partial).unwrap(), b"abc");
        assert!(!verify_whisper(&models, &artifact).unwrap());
    }

    #[test]
    fn oversized_and_full_corrupt_partials_restart_from_zero() {
        for contents in [b"abcdefgh".as_slice(), b"xxxxxx".as_slice()] {
            let fixture = FixtureDir::new();
            let models = fixture.models();
            let stage = models.join(STAGING_NAME);
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("fixture.bin.part"), contents).unwrap();
            let (base, server) = serve(vec![Reply {
                path: "/whisper",
                range: None,
                status: 200,
                content_range: None,
                declared_length: 6,
                body: b"abcdef",
            }]);
            let artifact = whisper("fixture.bin", format!("{base}/whisper"), b"abcdef");
            pull_fixture_whisper(&models, &artifact).unwrap();
            server.join().unwrap();
            assert!(verify_whisper(&models, &artifact).unwrap());
        }
    }

    #[test]
    fn offline_package_is_published_only_after_all_artifacts_verify() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        let (base, server) = serve(vec![
            Reply {
                path: "/first",
                range: None,
                status: 200,
                content_range: None,
                declared_length: 6,
                body: b"first!",
            },
            Reply {
                path: "/second",
                range: None,
                status: 200,
                content_range: None,
                declared_length: 6,
                body: b"se",
            },
        ]);
        let spec = offline([&format!("{base}/first"), &format!("{base}/second")]);
        assert!(pull_fixture_offline(&models, &spec).is_err());
        server.join().unwrap();
        assert!(!verify_offline(&models, &spec).unwrap());
        let stage = models.join(STAGING_NAME).join("fixture-offline.part");
        assert_eq!(std::fs::read(stage.join("first.bin")).unwrap(), b"first!");
        assert_eq!(std::fs::read(stage.join("second.bin.part")).unwrap(), b"se");

        let (base, server) = serve(vec![Reply {
            path: "/second",
            range: Some("bytes=2-"),
            status: 206,
            content_range: Some("bytes 2-5/6"),
            declared_length: 4,
            body: b"cond",
        }]);
        let spec = offline([&format!("{base}/first"), &format!("{base}/second")]);
        pull_fixture_offline(&models, &spec).unwrap();
        server.join().unwrap();
        assert!(verify_offline(&models, &spec).unwrap());
        assert!(!stage.exists());
    }

    #[test]
    fn offline_retry_cleans_owned_hardlink_partial_after_crash() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        let stage = models.join(STAGING_NAME).join("fixture-offline.part");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("first.bin"), b"first!").unwrap();
        std::fs::write(stage.join("first.bin.part"), b"first!").unwrap();
        let (base, server) = serve(vec![Reply {
            path: "/second",
            range: None,
            status: 200,
            content_range: None,
            declared_length: 6,
            body: b"second",
        }]);
        let spec = offline([&format!("{base}/first"), &format!("{base}/second")]);
        pull_fixture_offline(&models, &spec).unwrap();
        server.join().unwrap();
        assert!(verify_offline(&models, &spec).unwrap());
        assert!(!stage.exists());
    }

    #[test]
    fn healthy_offline_target_recovers_post_publish_stage() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        let spec = offline([
            "https://invalid.example/first",
            "https://invalid.example/second",
        ]);
        let target = models.join(&spec.directory);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("first.bin"), b"first!").unwrap();
        std::fs::write(target.join("second.bin"), b"second").unwrap();
        std::fs::write(target.join(NOTICE_NAME), NOTICE).unwrap();
        let staging = models.join(STAGING_NAME);
        let stage = staging.join("fixture-offline.part");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("first.bin"), b"xxxxxx").unwrap();
        pull_fixture_offline(&models, &spec).unwrap();
        assert!(!stage.exists());

        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("user-data.txt"), b"keep me").unwrap();
        pull_fixture_offline(&models, &spec).unwrap();
        assert!(!stage.exists());
        let preserved: Vec<_> = std::fs::read_dir(&staging)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("fixture-offline-preserved-")
            })
            .collect();
        assert_eq!(preserved.len(), 1);
        assert_eq!(
            std::fs::read(preserved[0].join("user-data.txt")).unwrap(),
            b"keep me"
        );
        assert!(verify_offline(&models, &spec).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn damaged_whisper_target_is_replaced_after_verified_download() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        std::fs::create_dir(&models).unwrap();
        std::fs::write(models.join("fixture.bin"), b"xxxxxx").unwrap();
        let (base, server) = serve(vec![Reply {
            path: "/whisper",
            range: None,
            status: 200,
            content_range: None,
            declared_length: 6,
            body: b"abcdef",
        }]);
        let artifact = whisper("fixture.bin", format!("{base}/whisper"), b"abcdef");
        pull_fixture_whisper(&models, &artifact).unwrap();
        server.join().unwrap();
        assert!(verify_whisper(&models, &artifact).unwrap());
        assert_eq!(
            std::fs::read(models.join("fixture.bin")).unwrap(),
            b"abcdef"
        );
    }

    #[test]
    fn verifies_hash_and_preserves_unknown_package_files_on_remove() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        std::fs::create_dir(&models).unwrap();
        let artifact = whisper(
            "fixture.bin",
            "https://invalid.example/file".into(),
            b"abcdef",
        );
        std::fs::write(models.join("fixture.bin"), b"xxxxxx").unwrap();
        assert!(verify_whisper(&models, &artifact).is_err());

        let spec = offline([
            "https://invalid.example/first",
            "https://invalid.example/second",
        ]);
        let package = models.join(&spec.directory);
        std::fs::create_dir(&package).unwrap();
        std::fs::write(package.join("first.bin"), b"first!").unwrap();
        std::fs::write(package.join("second.bin"), b"second").unwrap();
        std::fs::write(package.join(NOTICE_NAME), NOTICE).unwrap();
        std::fs::write(package.join("user-data.txt"), b"keep me").unwrap();
        assert!(verify_offline(&models, &spec).is_err());
        assert!(remove_offline(&models, &spec).is_err());
        assert_eq!(
            std::fs::read(package.join("user-data.txt")).unwrap(),
            b"keep me"
        );

        let mut traversal = artifact;
        traversal.filename = "../outside.bin".into();
        assert!(verify_whisper(&models, &traversal).is_err());
        assert!(remove_whisper(&models, &traversal).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_final_artifact_is_never_followed_or_removed() {
        use std::os::unix::fs::symlink;
        let fixture = FixtureDir::new();
        let models = fixture.models();
        std::fs::create_dir(&models).unwrap();
        let outside = fixture.0.join("outside.bin");
        std::fs::write(&outside, b"abcdef").unwrap();
        symlink(&outside, models.join("fixture.bin")).unwrap();
        let artifact = whisper(
            "fixture.bin",
            "https://invalid.example/file".into(),
            b"abcdef",
        );
        assert!(verify_whisper(&models, &artifact).is_err());
        assert!(remove_whisper(&models, &artifact).is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"abcdef");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn first_offline_publication_refuses_new_empty_target() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        std::fs::create_dir(&models).unwrap();
        let stage = models.join("stage");
        let target = models.join("target");
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("artifact.bin"), b"verified").unwrap();
        std::fs::create_dir(&target).unwrap();
        assert!(publish_offline_directory(&stage, &target).is_err());
        assert!(target.is_dir());
        assert_eq!(
            std::fs::read(stage.join("artifact.bin")).unwrap(),
            b"verified"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn damaged_offline_package_repairs_atomically_and_preserves_unknown_files() {
        let fixture = FixtureDir::new();
        let models = fixture.models();
        std::fs::create_dir(&models).unwrap();
        let target = models.join("fixture-offline");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("user-data.txt"), b"keep me").unwrap();
        let (base, server) = serve(vec![
            Reply {
                path: "/first",
                range: None,
                status: 200,
                content_range: None,
                declared_length: 6,
                body: b"first!",
            },
            Reply {
                path: "/second",
                range: None,
                status: 200,
                content_range: None,
                declared_length: 6,
                body: b"second",
            },
        ]);
        let spec = offline([&format!("{base}/first"), &format!("{base}/second")]);
        pull_fixture_offline(&models, &spec).unwrap();
        server.join().unwrap();
        assert!(verify_offline(&models, &spec).unwrap());
        let preserved: Vec<_> = std::fs::read_dir(models.join(STAGING_NAME))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("fixture-offline-preserved-")
            })
            .collect();
        assert_eq!(preserved.len(), 1);
        assert_eq!(
            std::fs::read(preserved[0].join("user-data.txt")).unwrap(),
            b"keep me"
        );
    }
}
