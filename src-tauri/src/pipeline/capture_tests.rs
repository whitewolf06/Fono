//! Capture fencing and final packet drain are tested without physical audio.
use super::Pipeline;
use crate::{
    audio::{AudioRecorder, RecordingWriter},
    error::{AppError, AppResult},
    operation::{OperationSource, TerminalReason},
};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
struct CaptureProbe {
    writer: Mutex<Option<RecordingWriter>>,
    starts: AtomicUsize,
    stops: AtomicUsize,
}
struct QueuedRecorder(Arc<CaptureProbe>);

impl AudioRecorder<RecordingWriter, AppError> for QueuedRecorder {
    fn start(
        &self,
        _: Option<&str>,
        writer: RecordingWriter,
        _: Arc<AtomicBool>,
        _: Arc<AtomicU32>,
        maximum: usize,
    ) -> AppResult<()> {
        let sequence = self.0.starts.fetch_add(1, Ordering::SeqCst) + 1;
        writer.lock().append(&[sequence as i16], maximum);
        *self.0.writer.lock() = Some(writer);
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        self.0.stops.fetch_add(1, Ordering::SeqCst);
        if let Some(writer) = self.0.writer.lock().take() {
            // The real AudioSubscription drains queued packets while dropping.
            writer.lock().append(&[7, 8], 100);
        }
        Ok(())
    }

    fn shutdown(&self) {}
}

fn fixture() -> (Pipeline, Arc<CaptureProbe>) {
    let probe = Arc::new(CaptureProbe::default());
    (
        Pipeline::new_with_audio_owner(Box::new(QueuedRecorder(probe.clone()))),
        probe,
    )
}

#[test]
fn old_stop_after_cancel_and_restart_cannot_detach_the_new_capture() {
    let (pipeline, probe) = fixture();
    pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let old = pipeline.operation_id();
    pipeline.cancel();
    assert!(pipeline.stop_capture_for(old).unwrap());
    pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let current = pipeline.operation_id();
    assert_ne!(old, current);

    assert!(pipeline.stop_recording_for(old).unwrap().is_none());
    assert!(!pipeline.stop_capture_for(old).unwrap());
    assert!(pipeline.is_recording());
    assert_eq!(pipeline.operation_id(), current);
    assert_eq!(pipeline.recording_end(), 1);
    assert_eq!(probe.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        pipeline.stop_recording_for(current).unwrap(),
        Some(vec![2, 7, 8])
    );
    assert_eq!(probe.stops.load(Ordering::SeqCst), 2);
}

#[test]
fn stop_capture_drains_once_and_collect_keeps_the_final_packets() {
    let (pipeline, probe) = fixture();
    pipeline
        .start_recording_from(None, OperationSource::Diagnostics)
        .unwrap();
    let operation = pipeline.operation_id();
    assert!(pipeline.stop_capture_for(operation).unwrap());
    assert!(!pipeline.is_recording());
    assert_eq!(pipeline.recording_end(), 3);
    assert!(pipeline.stop_capture_for(operation).unwrap());
    assert_eq!(probe.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        pipeline.stop_recording_for(operation).unwrap(),
        Some(vec![1, 7, 8])
    );
    assert!(pipeline.stop_recording_for(operation).unwrap().is_none());
    assert_eq!(probe.stops.load(Ordering::SeqCst), 1);
    pipeline.finish_operation(operation, TerminalReason::Completed);
}

#[test]
fn old_cancel_after_restart_preserves_new_capture_and_cancellation_token() {
    let (pipeline, probe) = fixture();
    pipeline
        .start_recording_from(None, OperationSource::WakeWord)
        .unwrap();
    let old = pipeline.operation_id();
    pipeline.cancel_for(old).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let current = pipeline.operation_id();
    let cancellation = pipeline.cancellation(current).unwrap();

    assert!(pipeline.cancel_for(old).is_none());
    assert!(pipeline.is_operation_active(current));
    assert!(!cancellation.is_cancelled());
    assert!(pipeline.is_recording());
    assert_eq!(probe.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        pipeline.stop_recording_for(current).unwrap(),
        Some(vec![2, 7, 8])
    );
}

#[tokio::test]
async fn cancelled_diagnostic_timer_cannot_collect_a_replacement_capture() {
    let (pipeline, probe) = fixture();
    pipeline
        .start_recording_from(None, OperationSource::Diagnostics)
        .unwrap();
    let old = pipeline.operation_id();
    let cancellation = pipeline.cancellation(old).unwrap();
    // Cancel while the diagnostic caller is waiting for its sample duration.
    let delayed = crate::application::dictation::collect_samples(
        &pipeline,
        old,
        std::time::Duration::from_millis(1),
        cancellation,
    );
    pipeline.cancel_for(old).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let current = pipeline.operation_id();
    assert!(matches!(delayed.await, Err(AppError::Cancelled(_))));
    assert!(pipeline.is_recording());
    assert!(pipeline.is_operation_active(current));
    assert_eq!(probe.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        pipeline.stop_recording_for(current).unwrap(),
        Some(vec![2, 7, 8])
    );
}

#[test]
fn stale_settings_cannot_replace_the_new_session_snapshot() {
    let (pipeline, _) = fixture();
    let old = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    let old_settings = crate::types::Settings {
        language: "ru".into(),
        ..crate::types::Settings::default()
    };
    assert!(pipeline.set_session_settings_for(old, old_settings.clone()));
    pipeline.cancel_for(old).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    let current = pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    assert!(pipeline.session_settings_for(current).is_none());
    let mut current_settings = old_settings.clone();
    current_settings.language = "en".into();
    assert!(pipeline.set_session_settings_for(current, current_settings));

    assert!(!pipeline.set_session_settings_for(old, old_settings));
    assert!(pipeline.session_settings_for(old).is_none());
    assert_eq!(
        pipeline.session_settings_for(current).unwrap().language,
        "en"
    );
    assert_eq!(pipeline.operation_id(), current);
    assert!(pipeline.is_recording());
}

#[test]
fn late_result_commit_does_not_run_after_cancel_and_replacement() {
    let (pipeline, _) = fixture();
    let old = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    assert_eq!(pipeline.while_operation(old, || "raw"), Some("raw"));
    pipeline.cancel_for(old).unwrap();
    pipeline.stop_recording_for(old).unwrap();
    let current = pipeline
        .start_recording_from(None, OperationSource::Hotkey)
        .unwrap();
    let cancellation = pipeline.cancellation(current).unwrap();
    let mut published = false;

    let result = pipeline.while_operation(old, || {
        published = true;
        "obsolete result"
    });
    assert!(result.is_none());
    assert!(!published);
    assert!(!cancellation.is_cancelled());
    assert!(pipeline.is_recording());
    assert_eq!(pipeline.operation_id(), current);
    assert_eq!(
        pipeline.while_operation(current, || "current result"),
        Some("current result")
    );
}
