use parking_lot::Mutex;

use crate::audio_source::AudioHub;
use crate::backend;
use crate::callback::CallbackSlot;
use crate::config::{WakeWordBackend, WakeWordConfig};
use crate::diag::Diagnostics;
use crate::error::WakeWordError;
use crate::error::WakeWordResult;
use crate::event::{capabilities_for_backend, WakeWordCapabilities, WakeWordEvent, WakeWordStatus};
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

    fn capabilities(&self) -> WakeWordCapabilities;

    /// Optional runtime diagnostics. Not every backend implements this.
    fn diagnostics(&self) -> Option<Diagnostics> {
        None
    }
}

pub fn validate_config(config: &WakeWordConfig) -> WakeWordResult<()> {
    if config.backend == WakeWordBackend::Disabled {
        return Ok(());
    }
    let capabilities = capabilities_for_backend(config.backend);
    if capabilities.supports_custom_phrase {
        return Ok(());
    }
    let requested = config.phrase.trim();
    if capabilities
        .supported_phrases
        .iter()
        .any(|phrase| phrase.eq_ignore_ascii_case(requested))
    {
        return Ok(());
    }
    Err(WakeWordError::Backend(format!(
        "backend {:?} does not support the wake phrase {:?}",
        config.backend, config.phrase
    )))
}

/// Thread-safe handle used by the main application.
pub struct WakeWordHandle {
    audio_hub: AudioHub,
    inner: Mutex<Inner>,
}

struct Inner {
    config: WakeWordConfig,
    engine: Option<Box<dyn WakeWordEngine>>,
    callback: CallbackSlot,
}

impl WakeWordHandle {
    pub fn new(config: WakeWordConfig) -> Self {
        Self::new_with_audio_hub(config, AudioHub::new())
    }

    pub fn new_with_audio_hub(config: WakeWordConfig, audio_hub: AudioHub) -> Self {
        let callback = CallbackSlot::default();
        let engine = if config.enabled {
            match build_engine(&config, audio_hub.clone()) {
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
            audio_hub,
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
        if !config.enabled {
            if let Some(engine) = inner.engine.as_mut() {
                let _ = engine.stop();
            }
            inner.config = config;
            inner.engine = None;
            inner.callback.notify(WakeWordEvent::Paused);
            return Ok(());
        }

        // Prepare and start the replacement before touching the currently
        // working engine. A model/audio failure therefore leaves the previous
        // runtime configuration active.
        let mut replacement = match build_engine(&config, self.audio_hub.clone())? {
            Some(e) => e,
            None => return Ok(()),
        };
        let cb = make_event_callback(&inner.callback);
        replacement.start(cb)?;

        if let Some(engine) = inner.engine.as_mut() {
            let _ = engine.stop();
        }
        inner.config = config;
        inner.engine = Some(replacement);
        Ok(())
    }

    pub fn start(&self) -> WakeWordResult<()> {
        let mut inner = self.inner.lock();
        if inner.engine.is_some() {
            return Ok(());
        }
        let config = inner.config.clone();
        let mut engine = match build_engine(&config, self.audio_hub.clone())? {
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

    pub fn capabilities(&self) -> WakeWordCapabilities {
        let inner = self.inner.lock();
        inner
            .engine
            .as_ref()
            .map(|engine| engine.capabilities())
            .unwrap_or_else(|| capabilities_for_backend(inner.config.backend))
    }
}

fn build_engine(
    config: &WakeWordConfig,
    audio_hub: AudioHub,
) -> WakeWordResult<Option<Box<dyn WakeWordEngine>>> {
    validate_config(config)?;
    #[cfg(not(any(feature = "whisper-wake", feature = "sherpa-wake")))]
    let _ = audio_hub;

    match config.backend {
        WakeWordBackend::Disabled => Ok(None),
        WakeWordBackend::Mock => Ok(Some(Box::new(backend::MockBackend::new(config.clone())))),
        WakeWordBackend::WhisperExperimental => {
            #[cfg(feature = "whisper-wake")]
            {
                Ok(Some(Box::new(backend::WhisperExperimentalBackend::new(
                    config.clone(),
                    audio_hub,
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
                    audio_hub,
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

#[cfg(test)]
mod tests {
    use crate::config::WakeWordConfig;
    use crate::event::WakeWordBackend;

    use super::validate_config;

    #[test]
    fn sherpa_rejects_a_phrase_outside_its_bundled_vocabulary() {
        let config = WakeWordConfig {
            backend: WakeWordBackend::SherpaOnnx,
            phrase: "привет фоно".into(),
            ..WakeWordConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn whisper_accepts_a_custom_phrase() {
        let config = WakeWordConfig {
            backend: WakeWordBackend::WhisperExperimental,
            phrase: "привет фоно".into(),
            ..WakeWordConfig::default()
        };
        assert!(validate_config(&config).is_ok());
    }
}
