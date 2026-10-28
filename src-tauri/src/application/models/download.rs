//! Desktop admission and progress adapters for shared model downloads.

use crate::{
    error::AppResult,
    events::{ModelDownloadEventV1, ModelDownloadPhaseV1},
};
use fono_stt::{
    models::download::{DownloadEvent, DownloadPhase, DownloadProgress},
    ports::ActivityLease,
};
use std::sync::{atomic::AtomicBool, Arc};
use tauri::AppHandle;

pub use fono_stt::models::download::{cancel_download, has_active_downloads};

pub(super) struct DownloadRegistration {
    _shared: fono_stt::models::download::DownloadRegistration,
    pub(super) cancellation: Arc<AtomicBool>,
}

impl DownloadRegistration {
    pub(super) fn begin(id: String) -> AppResult<Self> {
        let activity = crate::application::updates::activity::lease()?;
        let _configuration = crate::application::capture_configuration::begin_capture()?;
        let shared = fono_stt::models::download::DownloadRegistration::begin(
            id,
            ActivityLease::new(activity),
        )?;
        let cancellation = shared.cancellation.clone();
        Ok(Self {
            _shared: shared,
            cancellation,
        })
    }

    #[cfg(feature = "wake-word")]
    pub(super) fn blocking_lease(&self) -> ActivityLease {
        self._shared.blocking_lease()
    }
}

// Preserve whole-owner closure capture: moving only `cancellation` must not
// release the registration/admission while detached blocking work still runs.
impl Drop for DownloadRegistration {
    fn drop(&mut self) {}
}

pub(super) fn progress(app: AppHandle) -> impl DownloadProgress {
    move |event: DownloadEvent| {
        // Desktop announces admission before resolving its data directory.
        if event.phase == DownloadPhase::Started {
            return;
        }
        let phase = match event.phase {
            DownloadPhase::Started => ModelDownloadPhaseV1::Started,
            DownloadPhase::Downloading => ModelDownloadPhaseV1::Downloading,
            DownloadPhase::Verifying => ModelDownloadPhaseV1::Verifying,
            DownloadPhase::Extracting => ModelDownloadPhaseV1::Extracting,
            DownloadPhase::Completed => ModelDownloadPhaseV1::Completed,
            DownloadPhase::Cancelled => ModelDownloadPhaseV1::Cancelled,
            DownloadPhase::Failed => ModelDownloadPhaseV1::Failed,
        };
        emit_download(
            &app,
            &event.download_id,
            &event.model,
            phase,
            event.downloaded_bytes,
            event.total_bytes,
        );
    }
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
            download_id: download_id.into(),
            model: model.into(),
            phase,
            downloaded_bytes,
            total_bytes,
        },
    );
}
