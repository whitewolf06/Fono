use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, ThreadId},
    time::Duration,
};

use parking_lot::Mutex;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::{run, run_latest, Scheduler, UiTask};
use crate::{
    error::{AppError, AppResult},
    overlay::{
        clear_preview_if_current, invalidate_preview, position::Queue, preview_ticket_current,
        reserve_preview, visibility::Snapshot, OverlayPreview, OverlayRuntime,
    },
    types::PipelineState,
};

struct QueuedUi(UnboundedSender<(ThreadId, UiTask)>);

impl Scheduler for QueuedUi {
    fn submit(&self, task: UiTask) -> AppResult<()> {
        self.0
            .send((thread::current().id(), task))
            .map_err(|_| AppError::Internal("test UI queue closed".into()))
    }
}

async fn next(receiver: &mut UnboundedReceiver<(ThreadId, UiTask)>) -> (ThreadId, UiTask) {
    tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .expect("UI submit deadline")
        .expect("UI task queued")
}

#[tokio::test]
async fn dispatcher_defers_ui_work_and_releases_domain_locks_before_callback() {
    let ui_thread = thread::current().id();
    let capture = Arc::new(Mutex::new(()));
    let geometry = Arc::new(Mutex::new(()));
    let ran = Arc::new(AtomicBool::new(false));
    let (locked_sender, locked_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let owner_capture = capture.clone();
    let capture_owner = thread::spawn(move || {
        let guard = owner_capture.lock();
        locked_sender.send(()).unwrap();
        release_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        drop(guard);
    });
    locked_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let (sender, mut receiver) = unbounded_channel();
    let work_capture = capture.clone();
    let work_geometry = geometry.clone();
    let work_ran = ran.clone();
    // This future starts on the current-thread runtime, representing a command
    // issued by the UI while the atomic domain commit still owns its fence.
    let result = tokio::spawn(run(QueuedUi(sender), move || {
        assert_eq!(thread::current().id(), ui_thread);
        assert!(work_capture.try_lock().is_some());
        let _geometry = work_geometry.lock();
        // Window getters on this same UI thread execute inline, with no queue wait.
        work_ran.store(true, Ordering::Release);
        Ok(42)
    }));
    let (submit_thread, task) = next(&mut receiver).await;
    assert_ne!(
        submit_thread, ui_thread,
        "Tauri submission must not run inline on UI"
    );
    assert!(!ran.load(Ordering::Acquire));
    assert!(capture.try_lock().is_none());
    // The commit can finish without the queued UI task having executed.
    release_sender.send(()).unwrap();
    capture_owner.join().unwrap();
    task();
    assert_eq!(result.await.unwrap().unwrap(), 42);
    assert!(ran.load(Ordering::Acquire));
}

#[tokio::test]
async fn queued_idle_visibility_request_reads_new_dictation_pending_and_preview() {
    for current in [
        Snapshot {
            phase: PipelineState::Listening,
            enabled: true,
            pending: false,
            preview: false,
        },
        Snapshot {
            phase: PipelineState::Idle,
            enabled: false,
            pending: true,
            preview: false,
        },
        Snapshot {
            phase: PipelineState::Idle,
            enabled: false,
            pending: false,
            preview: true,
        },
        Snapshot {
            phase: PipelineState::Idle,
            enabled: true,
            pending: false,
            preview: false,
        },
    ] {
        let state = Arc::new(Mutex::new(Snapshot {
            phase: PipelineState::Idle,
            enabled: true,
            pending: false,
            preview: false,
        }));
        let (sender, mut receiver) = unbounded_channel();
        let read_state = state.clone();
        let result = tokio::spawn(run_latest(
            QueuedUi(sender),
            move || *read_state.lock(),
            |latest| Ok(latest.visible()),
        ));
        let (_, task) = next(&mut receiver).await;
        *state.lock() = current;
        task();
        assert_eq!(result.await.unwrap().unwrap(), current.visible());
    }
}

#[tokio::test]
async fn window_failure_reaches_the_async_command_without_blocking_ui() {
    let (sender, mut receiver) = unbounded_channel();
    let result = tokio::spawn(run(QueuedUi(sender), || -> AppResult<()> {
        Err(AppError::Internal("injected window failure".into()))
    }));
    let (_, task) = next(&mut receiver).await;
    task();
    assert!(result
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("injected window failure"));
}

#[tokio::test]
async fn discarded_ui_task_resolves_with_error_instead_of_waiting_forever() {
    let (sender, mut receiver) = unbounded_channel();
    let result = tokio::spawn(run(QueuedUi(sender), || Ok(())));
    let (_, task) = next(&mut receiver).await;
    drop(task);
    let error = tokio::time::timeout(Duration::from_secs(2), result)
        .await
        .expect("discarded callback deadline")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("overlay UI dispatch was closed"));
}

#[tokio::test]
async fn closed_ui_queue_returns_submit_error() {
    let (sender, receiver) = unbounded_channel();
    drop(receiver);
    let error = run(QueuedUi(sender), || Ok(())).await.unwrap_err();
    assert!(error.to_string().contains("test UI queue closed"));
}

#[test]
fn older_preview_expiry_cannot_clear_a_newer_preview() {
    let runtime = OverlayRuntime::default();
    *runtime.preview.lock() = Some(OverlayPreview {
        id: 2,
        overlay_scale: 1.0,
        overlay_opacity: 0.8,
        overlay_mini_mode: false,
    });
    assert!(!clear_preview_if_current(&runtime, 1));
    assert_eq!(runtime.preview.lock().as_ref().unwrap().id, 2);
    assert!(clear_preview_if_current(&runtime, 2));
    assert!(runtime.preview.lock().is_none());
}

#[test]
fn dismissal_invalidates_queued_show_and_pending_reset_preview() {
    let runtime = OverlayRuntime::default();
    let queued_show = reserve_preview(&runtime);
    let pending_reset = reserve_preview(&runtime);
    assert!(!preview_ticket_current(&runtime, queued_show));
    assert!(preview_ticket_current(&runtime, pending_reset));
    invalidate_preview(&runtime);
    assert!(!preview_ticket_current(&runtime, pending_reset));
    let explicit_new_preview = reserve_preview(&runtime);
    assert!(preview_ticket_current(&runtime, explicit_new_preview));
}

#[tokio::test]
async fn position_move_and_persist_are_owned_and_ordered_after_caller_cancellation() {
    let positions = Arc::new(Queue::default());
    let persisted = Arc::new(Mutex::new(Vec::new()));
    let (sender, mut receiver) = unbounded_channel();
    let (persist_started, persist_wait) = tokio::sync::oneshot::channel();
    let (finish_persist, persist_gate) = tokio::sync::oneshot::channel();

    let first_positions = positions.clone();
    let first_persisted = persisted.clone();
    let first_sender = sender.clone();
    let first = tokio::spawn(async move {
        first_positions
            .run(async move {
                run(QueuedUi(first_sender), || Ok(1)).await?;
                persist_started.send(()).unwrap();
                persist_gate.await.unwrap();
                first_persisted.lock().push(1);
                Ok(())
            })
            .await
    });
    let (_, first_move) = next(&mut receiver).await;
    first_move();
    persist_wait.await.unwrap();

    let second_positions = positions.clone();
    let second_persisted = persisted.clone();
    let second = tokio::spawn(async move {
        second_positions
            .run(async move {
                run(QueuedUi(sender), || Ok(2)).await?;
                second_persisted.lock().push(2);
                Ok(())
            })
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        receiver.try_recv().is_err(),
        "later move cannot precede earlier persistence"
    );
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    finish_persist.send(()).unwrap();
    let (_, second_move) = next(&mut receiver).await;
    assert_eq!(
        *persisted.lock(),
        vec![1],
        "cancelled caller's persistence still finishes"
    );
    second_move();
    second.await.unwrap().unwrap();
    assert_eq!(*persisted.lock(), vec![1, 2]);
}
