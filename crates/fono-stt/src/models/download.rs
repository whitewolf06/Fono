//! Download admission, cancellation, progress and bounded verified copying.

use crate::{
    error::{SttError as AppError, SttResult as AppResult},
    ports::ActivityLease,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const COPY_BUFFER_BYTES: usize = 64 * 1024;
const PROGRESS_STEP_BYTES: u64 = 1024 * 1024;

static ACTIVE_DOWNLOADS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// The host admits the operation before registering its model identifier.
/// Moving this owner into blocking work keeps admission until real completion.
pub struct DownloadRegistration {
    id: String,
    pub cancellation: Arc<AtomicBool>,
    activity: ActivityLease,
}

impl DownloadRegistration {
    pub fn begin(id: String, activity: ActivityLease) -> AppResult<Self> {
        let mut active = ACTIVE_DOWNLOADS.lock();
        if active.contains_key(&id) {
            return Err(AppError::Busy(format!(
                "model download {id} is already active"
            )));
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        active.insert(id.clone(), Arc::clone(&cancellation));
        Ok(Self {
            id,
            cancellation,
            activity,
        })
    }

    /// A detached native extraction may outlive its original async waiter.
    pub fn blocking_lease(&self) -> ActivityLease {
        self.activity.clone()
    }
}

impl Drop for DownloadRegistration {
    fn drop(&mut self) {
        ACTIVE_DOWNLOADS.lock().remove(&self.id);
    }
}

pub struct DownloadReceipt {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    Started,
    Downloading,
    Verifying,
    Extracting,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadEvent {
    pub download_id: String,
    pub model: String,
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

pub trait DownloadProgress: Send + Sync {
    fn report(&self, event: DownloadEvent);
}

impl<F: Fn(DownloadEvent) + Send + Sync> DownloadProgress for F {
    fn report(&self, event: DownloadEvent) {
        self(event);
    }
}

pub fn emit_download(
    progress: &dyn DownloadProgress,
    download_id: &str,
    model: &str,
    phase: DownloadPhase,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    progress.report(DownloadEvent {
        download_id: download_id.into(),
        model: model.into(),
        phase,
        downloaded_bytes,
        total_bytes,
    });
}

pub fn cancel_download(download_id: &str) -> bool {
    let Some(cancellation) = ACTIVE_DOWNLOADS.lock().get(download_id).cloned() else {
        return false;
    };
    cancellation.store(true, Ordering::Release);
    true
}

pub fn has_active_downloads() -> bool {
    !ACTIVE_DOWNLOADS.lock().is_empty()
}

pub fn download_client() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| AppError::Config(error.to_string()))
}

pub fn ensure_success(response: &reqwest::blocking::Response, label: &str) -> AppResult<()> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Config(format!(
            "HTTP {} while downloading {label}",
            response.status()
        )))
    }
}

pub fn copy_verified<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    expected_bytes: Option<u64>,
    cancellation: &AtomicBool,
    mut progress: F,
) -> AppResult<DownloadReceipt>
where
    R: Read,
    W: Write,
    F: FnMut(u64),
{
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut bytes = 0_u64;
    let mut next_progress = PROGRESS_STEP_BYTES;
    let mut hasher = Sha256::new();
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("model download cancelled".into()));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if expected_bytes.is_some_and(|expected| bytes.saturating_add(read as u64) > expected) {
            return Err(AppError::Config(
                "Model download exceeds the pinned artifact size".into(),
            ));
        }
        writer.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
        if bytes >= next_progress {
            progress(bytes);
            next_progress = bytes.saturating_add(PROGRESS_STEP_BYTES);
        }
    }
    if bytes == 0 {
        return Err(AppError::Config(
            "model download returned an empty file".into(),
        ));
    }
    if let Some(expected) = expected_bytes {
        if expected != bytes {
            return Err(AppError::Config(format!(
                "download size mismatch: expected {expected} bytes, got {bytes}"
            )));
        }
    }
    progress(bytes);
    Ok(DownloadReceipt {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn verified_copy_reports_size_and_checksum() {
        let input = b"verified model bytes";
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let receipt = copy_verified(
            &mut reader,
            &mut output,
            Some(input.len() as u64),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(receipt.bytes, input.len() as u64);
        assert_eq!(output, input);
        assert_eq!(receipt.sha256.len(), 64);
    }

    #[test]
    fn verified_copy_rejects_wrong_size() {
        let mut reader = Cursor::new(b"short");
        let mut output = Vec::new();
        assert!(copy_verified(
            &mut reader,
            &mut output,
            Some(10),
            &AtomicBool::new(false),
            |_| {},
        )
        .is_err());
    }
    #[test]
    fn pinned_size_bounds_output_before_an_oversized_response_is_written() {
        let mut reader = Cursor::new(vec![b'x'; 128 * 1024]);
        let mut output = Vec::new();
        assert!(copy_verified(
            &mut reader,
            &mut output,
            Some(5),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn verified_copy_honors_cancellation_before_writing() {
        let mut reader = Cursor::new(b"model");
        let mut output = Vec::new();
        assert!(matches!(
            copy_verified(
                &mut reader,
                &mut output,
                None,
                &AtomicBool::new(true),
                |_| {},
            ),
            Err(AppError::Cancelled(_))
        ));
        assert!(output.is_empty());
    }
}

#[cfg(test)]
#[path = "download_registry_tests.rs"]
mod registry_tests;
