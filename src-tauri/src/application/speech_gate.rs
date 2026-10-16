//! Speech decisions consume the whole ordered PCM stream, not the last RMS.
use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Manager};

pub fn model_path(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    let packaged = app.path().resolve(
        "resources/vad/silero_vad.onnx",
        tauri::path::BaseDirectory::Resource,
    )?;
    if packaged.is_file() {
        return Ok(packaged);
    }
    #[cfg(debug_assertions)]
    {
        let source =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/vad/silero_vad.onnx");
        if source.is_file() {
            return Ok(source);
        }
    }
    Err(AppError::Config(
        "Не найдена модель определения речи. Восстановите ресурсы Fono".into(),
    ))
}

pub struct SpeechGate {
    #[cfg(feature = "speech-vad")]
    inner: fono_audio::StreamingSpeechVad,
    #[cfg(not(feature = "speech-vad"))]
    unavailable: std::convert::Infallible,
}
pub struct SpeechDecision {
    pub speaking: bool,
    pub has_speech: bool,
    pub last_speech: u64,
    pub processed: u64,
}

impl SpeechGate {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        #[cfg(feature = "speech-vad")]
        {
            Ok(Self {
                inner: fono_audio::StreamingSpeechVad::new(&model_path(app)?)?,
            })
        }
        #[cfg(not(feature = "speech-vad"))]
        {
            let _ = app;
            Err(AppError::Config(
                "Речевой VAD отсутствует в этой сборке".into(),
            ))
        }
    }
    pub fn accept(&mut self, samples: &[i16]) -> SpeechDecision {
        #[cfg(feature = "speech-vad")]
        {
            let state = self.inner.accept(samples);
            SpeechDecision {
                speaking: state.current_speech,
                has_speech: state.has_speech,
                last_speech: state.last_speech_sample.unwrap_or(0),
                processed: state.processed_samples,
            }
        }
        #[cfg(not(feature = "speech-vad"))]
        {
            let _ = samples;
            match self.unavailable {}
        }
    }

    /// A completed capture needs a confirmed neural segment and sustained
    /// acoustic evidence. The streaming latch alone includes VAD hangover.
    pub fn recording_has_speech(&mut self, samples: &[i16], cancelled: impl Fn() -> bool) -> bool {
        #[cfg(feature = "speech-vad")]
        {
            self.inner
                .analyze_recording(samples, cancelled)
                .is_some_and(|decision| decision.has_speech)
        }
        #[cfg(not(feature = "speech-vad"))]
        {
            let _ = (samples, cancelled);
            match self.unavailable {}
        }
    }
}
