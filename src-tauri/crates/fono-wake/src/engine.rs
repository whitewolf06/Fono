use parking_lot::Mutex;

use crate::backend;
use crate::callback::CallbackSlot;
use crate::config::{WakeWordBackend, WakeWordConfig};
use crate::diag::Diagnostics;
#[cfg(any(not(feature = "whisper-wake"), not(feature = "sherpa-wake")))]
use crate::error::WakeWordError;
use crate::error::WakeWordResult;
use crate::event::{WakeWordEvent, WakeWordStatus};
use crate::WakeCallback;

/// Backend-agnostic wake word engine.
///
/// Backends are selected via `WakeWordConfig::backend`. The engine runs in its
/// own thread and emits events through the supplied callback.
pub trait WakeWordEngine: Send {
    fn start(&mut self, callback: WakeCallback) -> WakeWordResult<()>;
    fn stop(&mut self) -> WakeWordResult<()>;
    fn pause(&mut self) -> WakeWordResult<()>;
    fn resume(&mut self) -> WakeWordResult<()>;
    fn status(&self) -> WakeWordStatus;

    /// Optional runtime diagnostics. Not every backend implements this.
    fn diagnostics(&self) -> Option<Diagnostics> {
        None
    }
}

/// Thread-safe handle used by the main application.
pub struct WakeWordHandle {
    inner: Mutex<Inner>,
}

struct Inner {
    config: WakeWordConfig,
    engine: Option<Box<dyn WakeWordEngine>>,
    callback: CallbackSlot,
}

impl WakeWordHandle {
    pub fn new(config: WakeWordConfig) -> Self {
        let callback = CallbackSlot::default();
        let engine = if config.enabled {
            match build_engine(&config) {
                Ok(Some(mut engine)) => {
                    let cb = make_event_callback(&callback);
                    match engine.start(cb) {
                        Ok(()) => Some(engine),
                        Err(err) => {
                            tracing::error!("fono-wake: failed to start engine: {err}");
                            callback.notify(WakeWordEvent::Error {
                                message: err.to_string(),
                            });
                            None
                        }
                    }
                }
                Ok(None) => None,
                Err(err) => {
                    tracing::error!("fono-wake: failed to build engine: {err}");
                    callback.notify(WakeWordEvent::Error {
                        message: err.to_string(),
                    });
                    None
                }
            }
        } else {
            None
        };

        Self {
            inner: Mutex::new(Inner {
                config,
                engine,
                callback,
            }),
        }
    }

    /// Register the callback that receives wake word events in the main app.
    pub fn set_callback<F>(&self, callback: F)
    where
        F: Fn(WakeWordEvent) + Send + Sync + 'static,
    {
        let inner = self.inner.lock();
        inner.callback.set(std::sync::Arc::new(callback));
    }

    /// Replace configuration and restart the engine if wake word is enabled.
    pub fn update_config(&self, config: WakeWordConfig) -> WakeWordResult<()> {
        let mut inner = self.inner.lock();
        if let Some(engine) = inner.engine.as_mut() {
            let _ = engine.stop();
        }
        inner.config = config.clone();
        inner.engine = None;

        if !config.enabled {
            inner.callback.notify(WakeWordEvent::Paused);
            return Ok(());
        }

        let mut engine = match build_engine(&config)? {
            Some(e) => e,
            None => {
                inner.callback.notify(WakeWordEvent::Paused);
                return Ok(());
            }
        };

        let cb = make_event_callback(&inner.callback);
        engine.start(cb)?;
        inner.engine = Some(engine);
        Ok(())
    }

    pub fn start(&self) -> WakeWordResult<()> {
        let mut inner = self.inner.lock();
        if inner.engine.is_some() {
            return Ok(());
        }
        let config = inner.config.clone();
        let mut engine = match build_engine(&config)? {
            Some(e) => e,
            None => return Ok(()),
        };
        let cb = make_event_callback(&inner.callback);
        engine.start(cb)?;
        inner.engine = Some(engine);
        Ok(())
    }

    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        if let Some(engine) = inner.engine.as_mut() {
            let _ = engine.stop();
        }
        inner.engine = None;
    }

    pub fn pause(&self) {
        let mut inner = self.inner.lock();
        if let Some(engine) = inner.engine.as_mut() {
            let _ = engine.pause();
        }
    }

    pub fn resume(&self) {
        let mut inner = self.inner.lock();
        if let Some(engine) = inner.engine.as_mut() {
            let _ = engine.resume();
        }
    }

    pub fn status(&self) -> WakeWordStatus {
        let inner = self.inner.lock();
        match inner.engine.as_ref() {
            Some(e) => e.status(),
            None => WakeWordStatus::Off,
        }
    }

    pub fn config(&self) -> WakeWordConfig {
        self.inner.lock().config.clone()
    }

    pub fn diagnostics(&self) -> Option<Diagnostics> {
        let inner = self.inner.lock();
        inner.engine.as_ref().and_then(|e| e.diagnostics())
    }
}

fn build_engine(config: &WakeWordConfig) -> WakeWordResult<Option<Box<dyn WakeWordEngine>>> {
    match config.backend {
        WakeWordBackend::Disabled => Ok(None),
        WakeWordBackend::Mock => Ok(Some(Box::new(backend::MockBackend::new(config.clone())))),
        WakeWordBackend::WhisperExperimental => {
            #[cfg(feature = "whisper-wake")]
            {
                Ok(Some(Box::new(backend::WhisperExperimentalBackend::new(
                    config.clone(),
                ))))
            }
            #[cfg(not(feature = "whisper-wake"))]
            Err(WakeWordError::BackendNotCompiled(config.backend))
        }
        WakeWordBackend::SherpaOnnx => {
            #[cfg(feature = "sherpa-wake")]
            {
                Ok(Some(Box::new(backend::SherpaOnnxBackend::new(
                    config.clone(),
                ))))
            }
            #[cfg(not(feature = "sherpa-wake"))]
            Err(WakeWordError::BackendNotCompiled(config.backend))
        }
    }
}

fn make_event_callback(callback: &CallbackSlot) -> WakeCallback {
    let cb = callback.clone();
    std::sync::Arc::new(move |event| cb.notify(event))
}

impl Default for WakeWordHandle {
    fn default() -> Self {
        Self::new(WakeWordConfig::default())
    }
}
