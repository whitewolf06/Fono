//! Application use case for recording and recognizing diagnostic wake samples.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

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

/// PCM captured by the shared diagnostic recorder. Callers must consume or
/// drop `samples` within their use case; calibration never writes it to the
/// legacy diagnostic buffer.
pub(crate) struct RecordedWakeSample {
    pub samples: Vec<i16>,
    pub report: WakeWordSampleReport,
}

/// Records a user-controlled wake-word sample through the shared audio owner.
/// The live detector is paused so the phrase cannot trigger a real command.
pub async fn record_sample(app: AppHandle, duration_ms: u64) -> AppResult<WakeWordSampleReport> {
    let recorded = record_transient_sample(app, duration_ms).await?;
    *TEST_AUDIO.lock() = recorded.samples;
    Ok(recorded.report)
}

/// Records a sample without retaining it in the legacy diagnostic buffer.
/// The caller owns the returned audio and is responsible for dropping it after
/// its immediate operation finishes.
pub(crate) async fn record_transient_sample(
    app: AppHandle,
    duration_ms: u64,
) -> AppResult<RecordedWakeSample> {
    crate::types::ensure_wake_available()?;
    let settings = app.state::<AppState>().settings();
    let samples = super::dictation::record_diagnostic_sample(
        &app,
        settings.audio_device_id.as_deref(),
        std::time::Duration::from_millis(duration_ms.clamp(1_000, 10_000)),
    )
    .await?;
    if samples.is_empty() {
        return Err(AppError::Audio("тестовая запись пуста".into()));
    }

    let (rms, peak) = normalized_levels(&samples);
    Ok(RecordedWakeSample {
        report: WakeWordSampleReport {
            samples: samples.len(),
            duration_ms: samples.len() as u64 * 1_000 / 16_000,
            rms,
            peak,
        },
        samples,
    })
}

/// Runs the saved microphone sample through the selected wake-word backend.
pub async fn recognize_sample(app: AppHandle) -> AppResult<WakeWordRecognitionReport> {
    crate::types::ensure_wake_available()?;
    super::dictation::ensure_capture_allowed(&app)?;
    let samples = std::mem::take(&mut *TEST_AUDIO.lock());
    if samples.is_empty() {
        return Err(AppError::Audio("сначала запишите тестовую фразу".into()));
    }
    recognize_transient_samples(&app, samples).await
}

/// Runs an immediately-owned sample through the selected wake backend without
/// retaining it in the diagnostic buffer. The calibration and validation flows
/// use this path so their audio exists only for the active command.
pub(crate) async fn recognize_transient_samples(
    app: &AppHandle,
    samples: Vec<i16>,
) -> AppResult<WakeWordRecognitionReport> {
    crate::types::ensure_wake_available()?;
    let _activity = crate::application::updates::activity::lease()?;
    super::dictation::ensure_capture_allowed(app)?;
    let settings = app.state::<AppState>().settings();
    let config = crate::settings_to_wake_config(&settings)?;
    let backend = match settings.wake_backend {
        fono_wake::WakeWordBackend::WhisperExperimental => "Whisper Small",
        fono_wake::WakeWordBackend::SherpaOnnx => "Sherpa-ONNX",
        fono_wake::WakeWordBackend::SherpaStreamingRu => "Sherpa T-one (RU)",
        fono_wake::WakeWordBackend::SherpaStreamingEn => "Sherpa Zipformer (EN)",
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
        fono_wake::WakeWordBackend::SherpaOnnx
        | fono_wake::WakeWordBackend::SherpaStreamingRu
        | fono_wake::WakeWordBackend::SherpaStreamingEn => {
            recognize_sherpa_sample(config, samples).await?
        }
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

pub(crate) fn normalized_levels(samples: &[i16]) -> (f32, f32) {
    let mut sum = 0.0_f64;
    let mut peak = 0.0_f32;
    for &sample in samples {
        let normalized = sample as f32 / i16::MAX as f32;
        sum += (normalized as f64) * (normalized as f64);
        peak = peak.max(normalized.abs());
    }
    (((sum / samples.len().max(1) as f64) as f32).sqrt(), peak)
}
#[cfg(feature = "whisper-wake")]
async fn recognize_whisper_sample(
    config: fono_wake::WakeWordConfig,
    samples: Vec<i16>,
) -> AppResult<fono_wake::WakeWordTestResult> {
    let activity = crate::application::updates::activity::lease()?;
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
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
    let activity = crate::application::updates::activity::lease()?;
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let report = fono_wake::replay::replay_samples(&config, &samples)?;
        Ok::<_, fono_wake::WakeWordError>(fono_wake::WakeWordTestResult {
            detected: report.detections > 0,
            keyword: if report.detections > 0 {
                config.phrase
            } else {
                String::new()
            },
            json: serde_json::to_string(&report).unwrap_or_default(),
            samples: report.audio_samples,
            duration_ms: report.audio_duration_ms,
        })
    })
    .await
    .map_err(|error| AppError::Internal(format!("wake test join: {error}")))??)
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
