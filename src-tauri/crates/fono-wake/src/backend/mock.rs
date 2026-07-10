use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use crate::config::WakeWordConfig;
use crate::engine::WakeWordEngine;
use crate::event::{WakeWordEvent, WakeWordStatus};
use crate::error::WakeWordResult;

/// Simulated wake word for tests and UI demos.
///
/// Emits `Detected` on a fixed interval and toggles status between
/// `Listening` and `Detected`.
pub struct MockBackend {
    config: WakeWordConfig,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    status: Arc<Mutex<WakeWordStatus>>,
    callback: Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
}

impl MockBackend {
    pub fn new(config: WakeWordConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            thread: None,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            callback: Arc::new(Mutex::new(None)),
        }
    }
}

impl WakeWordEngine for MockBackend {
    fn start(&mut self, callback: Box<dyn Fn(WakeWordEvent) + Send>) -> WakeWordResult<()> {
        *self.callback.lock() = Some(callback);
        self.running.store(true, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Listening;
        let running = self.running.clone();
        let paused = self.paused.clone();
        let status = self.status.clone();
        let phrase = self.config.phrase.clone();
        let interval = Duration::from_millis(self.config.cooldown_ms.max(1));
        let cb = self.callback.clone();

        self.thread = Some(thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                thread::sleep(interval);
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if paused.load(Ordering::SeqCst) {
                    continue;
                }
                *status.lock() = WakeWordStatus::Processing;
                notify(
                    &cb,
                    WakeWordEvent::Detected {
                        phrase: phrase.clone(),
                        pre_roll: Vec::new(),
                    },
                );
                *status.lock() = WakeWordStatus::Listening;
            }
            *status.lock() = WakeWordStatus::Off;
        }));

        Ok(())
    }

    fn stop(&mut self) -> WakeWordResult<()> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        *self.status.lock() = WakeWordStatus::Off;
        Ok(())
    }

    fn pause(&mut self) -> WakeWordResult<()> {
        self.paused.store(true, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Paused;
        Ok(())
    }

    fn resume(&mut self) -> WakeWordResult<()> {
        self.paused.store(false, Ordering::SeqCst);
        *self.status.lock() = WakeWordStatus::Listening;
        Ok(())
    }

    fn status(&self) -> WakeWordStatus {
        *self.status.lock()
    }
}

fn notify(
    callback: &Arc<Mutex<Option<Box<dyn Fn(WakeWordEvent) + Send>>>>,
    event: WakeWordEvent,
) {
    if let Some(cb) = callback.lock().as_ref() {
        cb(event);
    }
}
