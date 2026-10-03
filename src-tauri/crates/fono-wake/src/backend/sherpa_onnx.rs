use super::sherpa_runtime::{run, Runtime};
use crate::{
    callback::CallbackSlot,
    diag::{self, Diagnostics, DiagnosticsHandle},
    event::capabilities_for_backend,
};
use crate::{
    AudioHub, WakeCallback, WakeWordCapabilities, WakeWordConfig, WakeWordEngine, WakeWordError,
    WakeWordEvent, WakeWordResult, WakeWordStatus,
};
use crossbeam_channel::bounded;
use parking_lot::Mutex;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub struct SherpaOnnxBackend {
    config: WakeWordConfig,
    audio_hub: AudioHub,
    status: Arc<Mutex<WakeWordStatus>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    reset: Arc<AtomicU64>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    callback: CallbackSlot,
    diag: DiagnosticsHandle,
}
impl SherpaOnnxBackend {
    pub fn new(config: WakeWordConfig, audio_hub: AudioHub) -> Self {
        Self {
            config,
            audio_hub,
            status: Arc::new(Mutex::new(WakeWordStatus::Off)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            reset: Arc::new(AtomicU64::new(0)),
            thread: Mutex::new(None),
            callback: CallbackSlot::default(),
            diag: diag::new_handle(),
        }
    }
}
impl WakeWordEngine for SherpaOnnxBackend {
    fn start(&mut self, callback: WakeCallback) -> WakeWordResult<()> {
        if self.running.load(Ordering::Acquire) {
            return Ok(());
        }
        self.callback.set(callback);
        if let Some(spec) = crate::model_spec(self.config.backend) {
            if let Some(path) = spec
                .files
                .iter()
                .map(|name| self.config.model_dir.join(name))
                .find(|path| !path.is_file())
            {
                *self.status.lock() = WakeWordStatus::MissingModel;
                self.callback.notify(WakeWordEvent::MissingModel {
                    path: path.to_string_lossy().into_owned(),
                });
                return Err(WakeWordError::ModelNotFound(path));
            }
        }
        *self.status.lock() = WakeWordStatus::Loading;
        self.callback.notify(WakeWordEvent::ModelLoading);
        self.running.store(true, Ordering::Release);
        self.paused.store(false, Ordering::Release);
        let runtime = Runtime {
            config: self.config.clone(),
            hub: self.audio_hub.clone(),
            status: self.status.clone(),
            running: self.running.clone(),
            paused: self.paused.clone(),
            reset: self.reset.clone(),
            callback: self.callback.clone(),
            diag: self.diag.clone(),
        };
        let (ready_tx, ready_rx) = bounded(1);
        let thread = thread::spawn(move || {
            let mut failed = false;
            if let Err(error) = run(&runtime, &ready_tx) {
                failed = true;
                let message = error.to_string();
                let _ = ready_tx.try_send(Err(message.clone()));
                runtime.callback.notify(WakeWordEvent::Error { message });
            }
            runtime.running.store(false, Ordering::Release);
            *runtime.status.lock() = if failed {
                WakeWordStatus::Error
            } else {
                WakeWordStatus::Off
            };
            diag::set_running(&runtime.diag, false);
        });
        *self.thread.lock() = Some(thread);
        match ready_rx.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => {
                self.stop()?;
                Err(WakeWordError::Backend(message))
            }
            Err(_) => {
                self.stop()?;
                Err(WakeWordError::Backend(
                    "wake model initialization timed out".into(),
                ))
            }
        }
    }
    fn stop(&mut self) -> WakeWordResult<()> {
        self.running.store(false, Ordering::Release);
        if let Some(t) = self.thread.lock().take() {
            let _ = t.join();
        }
        *self.status.lock() = WakeWordStatus::Off;
        diag::set_running(&self.diag, false);
        Ok(())
    }
    fn pause(&mut self) -> WakeWordResult<()> {
        self.paused.store(true, Ordering::Release);
        self.reset.fetch_add(1, Ordering::AcqRel);
        *self.status.lock() = WakeWordStatus::Paused;
        diag::set_paused(&self.diag, true);
        Ok(())
    }
    fn resume(&mut self) -> WakeWordResult<()> {
        self.reset.fetch_add(1, Ordering::AcqRel);
        self.paused.store(false, Ordering::Release);
        if self.running.load(Ordering::Acquire) {
            *self.status.lock() = WakeWordStatus::Listening;
        }
        diag::set_paused(&self.diag, false);
        Ok(())
    }
    fn status(&self) -> WakeWordStatus {
        *self.status.lock()
    }
    fn capabilities(&self) -> WakeWordCapabilities {
        capabilities_for_backend(self.config.backend)
    }
    fn diagnostics(&self) -> Option<Diagnostics> {
        Some(self.diag.lock().data.clone())
    }
}
impl Drop for SherpaOnnxBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
pub(crate) fn map_sensitivity(s: f32) -> f32 {
    0.5 + s.clamp(0.0, 1.0) * 3.5
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamps_sensitivity() {
        assert_eq!(map_sensitivity(-1.0), 0.5);
        assert_eq!(map_sensitivity(2.0), 4.0);
    }
}
