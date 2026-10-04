//! Terminal dismissal never cancels or hides a replacement capture.
use super::*;
use crate::{
    audio::{AudioRecorder, RecordingWriter},
    operation::OperationSource,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc, Arc,
};

struct FakeRecorder {
    dismissed: Arc<AtomicBool>,
}

impl AudioRecorder<RecordingWriter, AppError> for FakeRecorder {
    fn start(
        &self,
        _: Option<&str>,
        _: RecordingWriter,
        _: Arc<AtomicBool>,
        _: Arc<AtomicU32>,
        _: usize,
    ) -> AppResult<()> {
        assert!(self.dismissed.load(Ordering::Acquire));
        Ok(())
    }
    fn stop(&self) -> AppResult<()> {
        Ok(())
    }
    fn shutdown(&self) {}
}

#[test]
fn dismissal_queued_for_an_old_terminal_cannot_touch_the_new_operation() {
    let pipeline = Pipeline::new();
    let old = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    pipeline.cancel_for(old.id).unwrap();
    let successor = pipeline
        .operations
        .start(OperationSource::WakeWord)
        .unwrap();
    let cancellation = pipeline.cancellation(successor.id).unwrap();

    let result = pipeline.dismiss_overlay_when_idle(|| false, || panic!("must not hide"));

    assert!(matches!(result, Err(AppError::Busy(_))));
    assert_eq!(pipeline.current_operation().unwrap().id, successor.id);
    assert!(!cancellation.is_cancelled());
    assert!(!pipeline.current_operation().unwrap().confirmed);
}

#[test]
fn detached_coordinator_does_not_allow_dismissal_of_pending_or_capture_tail() {
    let pipeline = Pipeline::new();
    assert!(matches!(
        pipeline.dismiss_overlay_when_idle(|| true, || panic!("pending must not hide")),
        Err(AppError::Busy(_))
    ));

    // Cancellation releases the coordinator before capture cleanup finishes.
    let old = pipeline.operations.start(OperationSource::Hotkey).unwrap();
    *pipeline.recording.lock() = true;
    pipeline.capture_operation.store(old.id, Ordering::Release);
    pipeline.cancel_for(old.id).unwrap();
    assert!(pipeline.current_operation().is_none());
    assert!(matches!(
        pipeline.dismiss_overlay_when_idle(|| false, || panic!("recording must not hide")),
        Err(AppError::Busy(_))
    ));
    *pipeline.recording.lock() = false;
    assert!(matches!(
        pipeline.dismiss_overlay_when_idle(|| false, || panic!("tail must not hide")),
        Err(AppError::Busy(_))
    ));
    pipeline.capture_operation.store(0, Ordering::Release);
    pipeline
        .dismiss_overlay_when_idle(|| false, || Ok(()))
        .unwrap();
}

#[test]
fn accepted_dismissal_completes_before_a_concurrent_capture_can_attach() {
    let dismissed = Arc::new(AtomicBool::new(false));
    let pipeline = Arc::new(Pipeline::new_with_audio_owner(Box::new(FakeRecorder {
        dismissed: dismissed.clone(),
    })));
    let (attempted, attempted_rx) = mpsc::channel();
    let mut start = None;

    pipeline
        .dismiss_overlay_when_idle(
            || false,
            || {
                let successor = pipeline.clone();
                start = Some(std::thread::spawn(move || {
                    attempted.send(()).unwrap();
                    successor.start_recording_from(None, OperationSource::Hotkey)
                }));
                attempted_rx.recv().unwrap();
                assert!(pipeline.recording.try_lock().is_none());
                assert!(pipeline.current_operation().is_none());
                dismissed.store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap();

    start.unwrap().join().unwrap().unwrap();
    let successor = pipeline.current_operation().unwrap().id;
    assert!(pipeline.is_recording());
    assert!(pipeline.is_operation_active(successor));
    pipeline.cancel_for(successor).unwrap();
    pipeline.stop_recording_for(successor).unwrap();
}

#[test]
fn failed_hide_releases_the_idle_fence_without_cancelling_a_future_capture() {
    let pipeline = Pipeline::new();
    assert!(matches!(
        pipeline.dismiss_overlay_when_idle(
            || false,
            || Err(AppError::Internal("test window failure".into()))
        ),
        Err(AppError::Internal(_))
    ));
    assert!(pipeline.recording.try_lock().is_some());
    pipeline
        .dismiss_overlay_when_idle(|| false, || Ok(()))
        .unwrap();
    let next = pipeline.operations.start(OperationSource::Ui).unwrap();
    assert!(pipeline.is_operation_active(next.id));
}
