//! Application use case for recording and recognizing diagnostic wake samples.

#[cfg(feature = "sherpa-wake")]
use std::io::Write;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::operation::OperationSource;
use crate::pipeline::{self, Pipeline};
#[cfg(feature = "sherpa-wake")]
use crate::state;
use crate::state::AppState;
use crate::types::PipelineState;

static TEST_AUDIO: Lazy<Mutex<Vec<i16>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Debug, serde::Serialize)]
pub struct WakeWordSampleReport {
    pub samples: usize,
    pub duration_ms: u64,
    pub rms: f32,
    pub peak: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct WakeWordRecognitionReport {
    pub backend: String,
    pub detected: bool,
    pub recognized: String,
    pub json: String,
    pub audio_duration_ms: u64,
    pub processing_ms: u64,
}

/// Records a user-controlled wake-word sample through the shared audio owner.
/// The live detector is paused so the phrase cannot trigger a real command.
pub async fn record_sample(app: AppHandle, duration_ms: u64) -> AppResult<WakeWordSampleReport> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let wake_handle = app.state::<fono_wake::WakeWordHandle>();
    if pipeline.is_recording() {
        return Err(AppError::Audio("уже идёт другая запись".into()));
    }

    let settings = state.settings();
    wake_handle.pause();
    if let Err(error) = pipeline.start_recording_from(
        settings.audio_device_id.as_deref(),
        OperationSource::Diagnostics,
    ) {
        wake_handle.resume();
        return Err(error);
    }
    pipeline::set_state(&app, state.inner(), PipelineState::Listening);

    let duration = std::time::Duration::from_millis(duration_ms.clamp(1_000, 10_000));
    tokio::time::sleep(duration).await;
    let samples = pipeline.stop_recording();
    pipeline::set_state(&app, state.inner(), PipelineState::Idle);
    wake_handle.resume();
    let samples = samples?;
    if samples.is_empty() {
        return Err(AppError::Audio("тестовая запись пуста".into()));
    }

    let (rms, peak) = normalized_levels(&samples);
    *TEST_AUDIO.lock() = samples.clone();
    Ok(WakeWordSampleReport {
        samples: samples.len(),
        duration_ms: samples.len() as u64 * 1_000 / 16_000,
        rms,
        peak,
    })
}

/// Runs the saved microphone sample through the selected wake-word backend.
pub async fn recognize_sample(app: AppHandle) -> AppResult<WakeWordRecognitionReport> {
    let samples = std::mem::take(&mut *TEST_AUDIO.lock());
    if samples.is_empty() {
        return Err(AppError::Audio("сначала запишите тестовую фразу".into()));
    }
    let settings = app.state::<AppState>().settings();
    let config = crate::settings_to_wake_config(&settings)?;
    let backend = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => "Whisper Small",
        fono_wake::WakeWordBackend::SherpaOnnx => "Sherpa-ONNX",
        fono_wake::WakeWordBackend::Mock => "Mock",
        fono_wake::WakeWordBackend::Disabled => "Disabled",
    }
    .to_string();
    let audio_duration_ms = samples.len() as u64 * 1_000 / 16_000;
    let started = std::time::Instant::now();
    tracing::info!(
        backend = %backend,
        phrase = %settings.wake_word,
        samples = samples.len(),
        audio_duration_ms,
        "wake word recorded-sample recognition started"
    );

    let result = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => {
            recognize_whisper_sample(config, samples).await?
        }
        fono_wake::WakeWordBackend::SherpaOnnx => recognize_sherpa_sample(config, samples).await?,
        _ => {
            return Err(AppError::Internal(
                "тест записи поддерживается для Whisper и Sherpa-ONNX".into(),
            ));
        }
    };

    tracing::info!(
        backend = %backend,
        detected = result.detected,
        keyword = %result.keyword,
        processing_ms = started.elapsed().as_millis(),
        "wake word recorded-sample recognition finished"
    );
    Ok(WakeWordRecognitionReport {
        backend,
        detected: result.detected,
        recognized: result.keyword,
        json: result.json,
        audio_duration_ms,
        processing_ms: started.elapsed().as_millis() as u64,
    })
}

fn normalized_levels(samples: &[i16]) -> (f32, f32) {
    let mut sum = 0.0_f64;
    let mut peak = 0.0_f32;
    for &sample in samples {
        let normalized = sample as f32 / i16::MAX as f32;
        sum += (normalized as f64) * (normalized as f64);
        peak = peak.max(normalized.abs());
    }
    (((sum / samples.len().max(1) as f64) as f32).sqrt(), peak)
}

#[cfg(feature = "sherpa-wake")]
fn write_pcm16_wav(path: &std::path::Path, samples: &[i16], sample_rate: u32) -> AppResult<()> {
    let data_len = std::mem::size_of_val(samples) as u32;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * 2).to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(feature = "whisper-wake")]
async fn recognize_whisper_sample(
    config: fono_wake::WakeWordConfig,
    samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Ok(tauri::async_runtime::spawn_blocking(move || {
        fono_wake::test_whisper_with_samples(&config, &samples)
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??)
}

#[cfg(not(feature = "whisper-wake"))]
async fn recognize_whisper_sample(
    _config: fono_wake::WakeWordConfig,
    _samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Err(AppError::Internal(
        "whisper-wake backend не собран в эту сборку".into(),
    ))
}

#[cfg(feature = "sherpa-wake")]
async fn recognize_sherpa_sample(
    config: fono_wake::WakeWordConfig,
    samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    let wav_path = state::app_data_dir()?.join(format!(
        ".wake-word-test-{}-{}.wav",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let _cleanup = TemporaryFile::new(wav_path.clone());
    write_pcm16_wav(&wav_path, &samples, 16_000)?;
    Ok(tauri::async_runtime::spawn_blocking(move || {
        fono_wake::test_with_wav(&config, &wav_path, false)
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??)
}

#[cfg(feature = "sherpa-wake")]
struct TemporaryFile {
    path: std::path::PathBuf,
}

#[cfg(feature = "sherpa-wake")]
impl TemporaryFile {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(feature = "sherpa-wake")]
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = ?self.path, %error, "could not remove wake diagnostic WAV");
            }
        }
    }
}

#[cfg(not(feature = "sherpa-wake"))]
async fn recognize_sherpa_sample(
    _config: fono_wake::WakeWordConfig,
    _samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    Err(AppError::Internal(
        "sherpa-wake backend не собран в эту сборку".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::normalized_levels;

    #[test]
    fn normalized_levels_handle_empty_and_peak_samples() {
        assert_eq!(normalized_levels(&[]), (0.0, 0.0));
        let (rms, peak) = normalized_levels(&[i16::MAX, 0]);
        assert!((rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
        assert_eq!(peak, 1.0);
    }
}
