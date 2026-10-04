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
    #[cfg(feature = "sherpa-wake")]
    inner: fono_wake::StreamingSpeechVad,
}
pub struct SpeechDecision {
    pub speaking: bool,
    pub has_speech: bool,
    pub last_speech: u64,
    pub processed: u64,
}

impl SpeechGate {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        #[cfg(feature = "sherpa-wake")]
        {
            Ok(Self {
                inner: fono_wake::StreamingSpeechVad::new(&model_path(app)?)?,
            })
        }
        #[cfg(not(feature = "sherpa-wake"))]
        {
            let _ = app;
            Err(AppError::Config(
                "Речевой VAD отсутствует в этой сборке".into(),
            ))
        }
    }
    pub fn accept(&mut self, samples: &[i16]) -> SpeechDecision {
        #[cfg(feature = "sherpa-wake")]
        {
            let state = self.inner.accept(samples);
            SpeechDecision {
                speaking: state.current_speech,
                has_speech: state.has_speech,
                last_speech: state.last_speech_sample.unwrap_or(0),
                processed: state.processed_samples,
            }
        }
        #[cfg(not(feature = "sherpa-wake"))]
        {
            let _ = samples;
            SpeechDecision {
                speaking: false,
                has_speech: false,
                last_speech: 0,
                processed: 0,
            }
        }
    }

    /// A completed capture needs a confirmed neural segment and sustained
    /// acoustic evidence. The streaming latch alone includes VAD hangover.
    pub fn recording_has_speech(&mut self, samples: &[i16], cancelled: impl Fn() -> bool) -> bool {
        #[cfg(feature = "sherpa-wake")]
        {
            self.inner
                .analyze_recording(samples, cancelled)
                .is_some_and(|decision| decision.has_speech)
        }
        #[cfg(not(feature = "sherpa-wake"))]
        {
            let _ = (samples, cancelled);
            false
        }
    }
}
