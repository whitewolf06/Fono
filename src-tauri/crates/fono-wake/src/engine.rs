use std::sync::Arc;

use parking_lot::Mutex;

use crate::backend;
use crate::config::{WakeWordBackend, WakeWordConfig};
use crate::error::WakeWordResult;
use crate::event::{WakeWordEvent, WakeWordStatus};

/// Backend-agnostic wake word engine.
///
/// Backends are selected via `WakeWordConfig::backend`. The engine runs in its
/// own thread and emits events through the supplied callback.
pub trait WakeWordEngine: Send {
    fn start(&mut self, callback: Box<dyn Fn(WakeWordEvent) + Send>) -> WakeWordResult<()>;
    fn stop(&mut self) -> WakeWordResult<()>;
    fn pause(&mut self) -> WakeWordResult<()>;
    fn resume(&mut self) -> WakeWordResult<()>;
    fn status(&self) -> WakeWordStatus;
}

/// Thread-safe handle used by the main application.
pub struct WakeWordHandle {
    inner: Mutex<Inner>,
}

struct Inner {
    config: WakeWordConfig,
    engine: Option<Box<dyn WakeWordEngine>>,
    callback: Arc<Mutex<Option<Arc<dyn Fn(WakeWordEvent) + Send + Sync>>>>,
}

fn notify_handle(
    callback: &Arc<Mutex<Option<Arc<dyn Fn(WakeWordEvent) + Send + Sync>>>>,
    event: WakeWordEvent,
) {
    if let Some(cb) = callback.lock().as_ref() {
        cb(event);
    }
}

impl WakeWordHandle {
    pub fn new(config: WakeWordConfig) -> Self {
        let callback = Arc::new(Mutex::new(None));
        let engine = build_engine(&config, &callback);
        let engine = match engine {
            Ok(Some(mut e)) => {
                let cb = make_event_callback(&callback);
                match e.start(cb) {
                    Ok(()) => Some(e),
                    Err(err) => {
                        tracing::error!("fono-wake: failed to start engine: {err}");
                        notify_handle(&callback, WakeWordEvent::Error { message: err.to_string() });
                        None
                    }
                }
            }
            Ok(None) => None,
            Err(err) => {
                tracing::error!("fono-wake: failed to build engine: {err}");
                notify_handle(&callback, WakeWordEvent::Error { message: err.to_string() });
                None
            }
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
        *inner.callback.lock() = Some(Arc::new(callback));
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
            notify_handle(&inner.callback, WakeWordEvent::Paused);
            return Ok(());
        }

        let mut engine = match build_engine(&config, &inner.callback)? {
            Some(e) => e,
            None => {
                notify_handle(&inner.callback, WakeWordEvent::Paused);
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
        let mut engine = match build_engine(&config, &inner.callback)? {
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
}

fn build_engine(
    config: &WakeWordConfig,
    _callback: &Arc<Mutex<Option<Arc<dyn Fn(WakeWordEvent) + Send + Sync>>>>,
) -> WakeWordResult<Option<Box<dyn WakeWordEngine>>> {
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

fn make_event_callback(
    callback: &Arc<Mutex<Option<Arc<dyn Fn(WakeWordEvent) + Send + Sync>>>>,
) -> Box<dyn Fn(WakeWordEvent) + Send> {
    let cb = callback.clone();
    Box::new(move |event| notify_handle(&cb, event))
}

impl Default for WakeWordHandle {
    fn default() -> Self {
        Self::new(WakeWordConfig::default())
    }
}
