//! Desktop adapters for the shared in-memory transcription queue.

use fono_service::transcription_jobs::{CaptureGate, CaptureGuard};
use fono_stt::{
    error::{SttError, SttResult},
    ActivityGuard, ActivityLease,
};
use fono_types::TranscriptionServiceError;

pub use fono_service::transcription_jobs::{
    InteractiveActivity, JobState, NoCaptureGate, NoInteractiveActivity, TranscriptionJob,
    TranscriptionJobQueue, TranscriptionJobWorker, TranscriptionJobs, TranscriptionQueueSnapshot,
};

/// The queued job and its actual native work keep the desktop updater fence.
pub struct DesktopQueueActivity;

impl ActivityGuard for DesktopQueueActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        crate::application::updates::activity::lease()
            .map(ActivityLease::new)
            .map_err(SttError::from)
    }

    fn cleanup_lease(&self) -> ActivityLease {
        ActivityLease::new(crate::application::updates::activity::cleanup_lease())
    }
}

/// Capture admission stays atomic with queue insertion and dequeue.
pub struct DesktopCaptureGate;

impl CaptureGate for DesktopCaptureGate {
    fn begin(&self) -> Result<Box<dyn CaptureGuard>, TranscriptionServiceError> {
        let guard = crate::application::capture_configuration::begin_capture().map_err(|_| {
            TranscriptionServiceError::Busy("speech configuration is changing".into())
        })?;
        Ok(Box::new(guard))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_queue_activity_keeps_the_update_fence_until_last_owner_finishes() {
        let admitted = DesktopQueueActivity.lease().unwrap();
        let native_work = admitted.clone();
        drop(admitted);
        assert!(crate::application::updates::activity::is_busy());
        drop(native_work);
    }
}
