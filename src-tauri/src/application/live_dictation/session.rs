use crate::{
    injection::target::TextTarget,
    operation::{OperationCancellation, OperationSource},
    types::{Settings, Transcript},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct LiveSnapshot {
    pub session_id: String,
    pub revision: u64,
    pub committed_text: String,
    pub draft_text: String,
    pub insertion_state: String,
    pub pending_text: String,
    pub lag_ms: u64,
    pub phase: String,
    pub source: OperationSource,
    pub elapsed_ms: u64,
    pub audio_level: f32,
    pub warning: Option<String>,
}

pub(super) struct LiveData {
    pub snapshot: LiveSnapshot,
    pub target: Option<TextTarget>,
    pub decode_seconds: f32,
}
impl LiveData {
    pub fn new(operation: u64, source: OperationSource, target: Option<TextTarget>) -> Self {
        let insertion_state = if target.is_some() {
            "active"
        } else if source == OperationSource::Ui {
            "none"
        } else {
            "paused_focus"
        };
        Self {
            target,
            decode_seconds: 0.0,
            snapshot: LiveSnapshot {
                session_id: operation.to_string(),
                revision: 0,
                source,
                committed_text: String::new(),
                draft_text: String::new(),
                pending_text: String::new(),
                insertion_state: insertion_state.into(),
                lag_ms: 0,
                phase: "listening".into(),
                elapsed_ms: 0,
                audio_level: 0.0,
                warning: None,
            },
        }
    }
}
pub(super) struct LiveSession {
    pub operation: u64,
    pub settings: Settings,
    pub cancellation: OperationCancellation,
    pub started: std::time::Instant,
    pub stop: AtomicBool,
    pub finished: AtomicBool,
    pub terminal_started: AtomicBool,
    pub data: Mutex<LiveData>,
    pub insertion: tokio::sync::Mutex<()>,
    pub completion: tokio::sync::watch::Sender<Option<Result<Transcript, String>>>,
}
impl LiveSession {
    pub fn request_cancel(&self) -> bool {
        let _data = self.data.lock();
        if self.terminal_started.load(Ordering::Acquire) {
            return false;
        }
        self.cancellation.cancel();
        self.stop.store(true, Ordering::Release);
        true
    }

    pub fn seal_outcome(&self) -> bool {
        let _data = self.data.lock();
        self.terminal_started.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        self.cancellation.is_cancelled()
    }

    pub fn emit(&self, app: &AppHandle) {
        if !super::owns(app, self) {
            return;
        }
        let mut data = self.data.lock();
        data.snapshot.revision += 1;
        data.snapshot.elapsed_ms = self.started.elapsed().as_millis() as u64;
        let snapshot = data.snapshot.clone();
        drop(data);
        let _ = app.emit("dictation-live", snapshot);
    }
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> LiveSession {
        LiveSession {
            operation: 1,
            settings: Settings::default(),
            cancellation: OperationCancellation::default(),
            started: std::time::Instant::now(),
            stop: Default::default(),
            finished: Default::default(),
            terminal_started: Default::default(),
            data: Mutex::new(LiveData::new(1, OperationSource::Ui, None)),
            insertion: tokio::sync::Mutex::new(()),
            completion: tokio::sync::watch::channel(None).0,
        }
    }
    #[test]
    fn cancellation_precedes_sealing_or_preserves_completed_success() {
        let cancelled = session();
        assert!(cancelled.request_cancel());
        assert!(cancelled.seal_outcome());
        let completed = session();
        assert!(!completed.seal_outcome());
        assert!(!completed.request_cancel());
        assert!(!completed.cancellation.is_cancelled());
    }
    #[test]
    fn concurrent_cancel_and_seal_choose_one_consistent_outcome() {
        for _ in 0..100 {
            let session = std::sync::Arc::new(session());
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let other = session.clone();
            let signal = barrier.clone();
            let cancel = std::thread::spawn(move || {
                signal.wait();
                other.request_cancel()
            });
            barrier.wait();
            let sealed_cancelled = session.seal_outcome();
            assert_eq!(sealed_cancelled, cancel.join().unwrap());
            assert_eq!(sealed_cancelled, session.cancellation.is_cancelled());
        }
    }
}
