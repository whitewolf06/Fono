use crate::{
    error::{AppError, AppResult},
    events::{ModelDownloadEventV1, ModelDownloadPhaseV1},
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::AppHandle;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const PROGRESS_STEP_BYTES: u64 = 1024 * 1024;

static ACTIVE_DOWNLOADS: Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) struct DownloadRegistration {
    id: String,
    pub(super) cancellation: Arc<AtomicBool>,
    _activity: crate::application::updates::activity::ActivityLease<'static>,
}

impl DownloadRegistration {
    /// Blocking verification/extraction may outlive the owning IPC future.
    pub(super) fn blocking_lease(
        &self,
    ) -> crate::application::updates::activity::ActivityLease<'static> {
        self._activity.clone()
    }

    pub(super) fn begin(id: String) -> AppResult<Self> {
        let activity = crate::application::updates::activity::lease()?;
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
            _activity: activity,
        })
    }
}

impl Drop for DownloadRegistration {
    fn drop(&mut self) {
        ACTIVE_DOWNLOADS.lock().remove(&self.id);
    }
}

pub(super) struct DownloadReceipt {
    pub(super) bytes: u64,
    pub(super) sha256: String,
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

pub(super) fn download_client() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|error| AppError::Config(error.to_string()))
}

pub(super) fn ensure_success(response: &reqwest::blocking::Response, label: &str) -> AppResult<()> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Config(format!(
            "HTTP {} while downloading {label}",
            response.status()
        )))
    }
}

pub(super) fn copy_verified<R, W, F>(
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

pub(super) fn emit_download(
    app: &AppHandle,
    download_id: &str,
    model: &str,
    phase: ModelDownloadPhaseV1,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    crate::events::emit_model_download(
        app,
        ModelDownloadEventV1 {
            download_id: download_id.to_string(),
            model: model.to_string(),
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
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
