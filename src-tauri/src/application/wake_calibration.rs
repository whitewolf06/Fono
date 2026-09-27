//! Privacy-preserving local wake-word calibration.
//!
//! A calibration session retains aggregate quality metrics only. Each PCM
//! buffer is assessed in the recording command and immediately dropped before
//! the session state is updated or persisted.

use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::types::{Settings, WakeCalibrationProfile, WakeWordBackend};

pub const REQUIRED_SAMPLES: u8 = 10;
pub const SAMPLE_DURATION_MS: u64 = 3_000;
const SAMPLE_RATE: u64 = 16_000;
const SILENCE_RMS: f32 = 0.007;
const CLIPPING_PEAK: f32 = 0.985;
const ACTIVE_SIGNAL: f32 = 0.02;
const MIN_ACTIVE_MS: u64 = 300;
const SHERPA_MODEL_VERSION: &str = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01";

#[derive(Default)]
pub struct WakeCalibrationService {
    session: Mutex<Option<CalibrationSession>>,
    next_session_id: Mutex<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeCalibrationStatus {
    pub active: bool,
    pub recording: bool,
    pub required_samples: u8,
    pub accepted_samples: u8,
    pub rejected_samples: u8,
    pub phrase: String,
    pub latest_result: Option<WakeCalibrationSampleResult>,
    pub profile: Option<WakeCalibrationProfile>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeCalibrationSampleResult {
    pub accepted: bool,
    pub reason: Option<WakeCalibrationRejection>,
    pub rms: f32,
    pub peak: f32,
    pub active_ms: u64,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeCalibrationRejection {
    Silence,
    Clipping,
    TooShort,
}

#[derive(Clone)]
struct CalibrationContext {
    backend: WakeWordBackend,
    model_version: String,
    phrase: String,
    graph: String,
    threshold: f32,
    sensitivity: f32,
    vad_threshold: f32,
}

struct CalibrationSession {
    id: u64,
    context: CalibrationContext,
    recording: bool,
    accepted: Vec<AcceptedSample>,
    rejected_samples: u8,
    latest_result: Option<WakeCalibrationSampleResult>,
}

#[derive(Clone, Copy)]
struct AcceptedSample {
    rms: f32,
    peak: f32,
    active_ms: u64,
}

impl WakeCalibrationService {
    fn begin(&self, settings: &Settings) -> AppResult<()> {
        let context = calibration_context(settings)?;
        let mut next_session_id = self.next_session_id.lock();
        *next_session_id = next_session_id.wrapping_add(1).max(1);
        *self.session.lock() = Some(CalibrationSession {
            id: *next_session_id,
            context,
            recording: false,
            accepted: Vec::with_capacity(REQUIRED_SAMPLES as usize),
            rejected_samples: 0,
            latest_result: None,
        });
        Ok(())
    }

    fn begin_recording(&self) -> AppResult<u64> {
        let mut sessions = self.session.lock();
        let session = sessions
            .as_mut()
            .ok_or_else(|| AppError::Config("Сначала начните калибровку wake word".into()))?;
        if session.recording {
            return Err(AppError::Audio("Уже записывается следующий образец".into()));
        }
        if session.accepted.len() >= REQUIRED_SAMPLES as usize {
            return Err(AppError::Config("Калибровка уже завершена".into()));
        }
        session.recording = true;
        Ok(session.id)
    }

    fn recording_failed(&self, id: u64) {
        let mut sessions = self.session.lock();
        if let Some(session) = sessions.as_mut().filter(|session| session.id == id) {
            session.recording = false;
        }
    }

    fn record_result(
        &self,
        id: u64,
        result: WakeCalibrationSampleResult,
    ) -> Option<WakeCalibrationProfile> {
        let mut sessions = self.session.lock();
        let session = sessions.as_mut().filter(|session| session.id == id)?;
        session.recording = false;
        session.latest_result = Some(result.clone());
        if result.accepted {
            session.accepted.push(AcceptedSample {
                rms: result.rms,
                peak: result.peak,
                active_ms: result.active_ms,
            });
            if session.accepted.len() == REQUIRED_SAMPLES as usize {
                return Some(build_profile(
                    &session.context,
                    &session.accepted,
                    session.rejected_samples,
                ));
            }
        } else {
            session.rejected_samples = session.rejected_samples.saturating_add(1);
        }
        None
    }

    fn cancel(&self) -> AppResult<()> {
        let mut session = self.session.lock();
        if session.as_ref().is_some_and(|current| current.recording) {
            return Err(AppError::Audio(
                "Дождитесь завершения текущей записи перед отменой калибровки".into(),
            ));
        }
        // The session has no audio samples: clearing it only removes aggregate
        // metrics and makes a future registration start from 1/10.
        *session = None;
        Ok(())
    }

    fn status(&self, profile: Option<WakeCalibrationProfile>) -> WakeCalibrationStatus {
        let session = self.session.lock();
        let Some(session) = session.as_ref() else {
            return WakeCalibrationStatus {
                active: false,
                recording: false,
                required_samples: REQUIRED_SAMPLES,
                accepted_samples: 0,
                rejected_samples: 0,
                phrase: "рамзи".to_owned(),
                latest_result: None,
                profile,
            };
        };
        WakeCalibrationStatus {
            active: session.accepted.len() < REQUIRED_SAMPLES as usize,
            recording: session.recording,
            required_samples: REQUIRED_SAMPLES,
            accepted_samples: session.accepted.len() as u8,
            rejected_samples: session.rejected_samples,
            phrase: session.context.phrase.clone(),
            latest_result: session.latest_result.clone(),
            profile,
        }
    }
}

/// Begins a new registration and deliberately discards previous temporary
/// aggregate data. A completed profile remains usable until FONO-45 replaces
/// it with validated values.
pub fn start(app: &AppHandle) -> AppResult<WakeCalibrationStatus> {
    if crate::application::wake_validation::status(app).active {
        return Err(AppError::Config(
            "Сначала завершите или отмените проверку профиля wake word".into(),
        ));
    }
    let settings = app.state::<AppState>().settings();
    let service = app.state::<WakeCalibrationService>();
    service.begin(&settings)?;
    Ok(service.status(settings.wake_calibration_profile))
}

/// Captures one controlled sample. The live listener is paused inside the
/// shared recorder; this function never sends the PCM buffer to STT, history,
/// analytics, logs, or persistent settings.
pub async fn record_next(app: AppHandle) -> AppResult<WakeCalibrationStatus> {
    let service = app.state::<WakeCalibrationService>();
    let session_id = service.begin_recording()?;
    let recorded =
        match crate::application::wake::record_transient_sample(app.clone(), SAMPLE_DURATION_MS)
            .await
        {
            Ok(recorded) => recorded,
            Err(error) => {
                service.recording_failed(session_id);
                return Err(error);
            }
        };

    let result = assess_samples(&recorded.samples);
    drop(recorded);
    let completed_profile = service.record_result(session_id, result);
    if let Some(profile) = completed_profile {
        persist_profile(&app, profile)?;
    }
    let settings = app.state::<AppState>().settings();
    Ok(service.status(settings.wake_calibration_profile))
}

pub fn cancel(app: &AppHandle) -> AppResult<WakeCalibrationStatus> {
    let service = app.state::<WakeCalibrationService>();
    service.cancel()?;
    let profile = app.state::<AppState>().settings().wake_calibration_profile;
    Ok(service.status(profile))
}

pub fn status(app: &AppHandle) -> WakeCalibrationStatus {
    let profile = app.state::<AppState>().settings().wake_calibration_profile;
    app.state::<WakeCalibrationService>().status(profile)
}

fn persist_profile(app: &AppHandle, profile: WakeCalibrationProfile) -> AppResult<()> {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    settings.wake_calibration_profile = Some(profile);
    state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    crate::events::emit_settings(app, &settings);
    Ok(())
}

fn calibration_context(settings: &Settings) -> AppResult<CalibrationContext> {
    if settings.wake_backend != WakeWordBackend::SherpaOnnx {
        return Err(AppError::Config(
            "Калибровка «рамзи» сейчас доступна только для Sherpa-ONNX".into(),
        ));
    }
    if !settings.wake_word.trim().eq_ignore_ascii_case("рамзи") {
        return Err(AppError::Config(
            "Перед калибровкой выберите и сохраните ключевую фразу «рамзи»".into(),
        ));
    }
    let config = crate::settings_to_wake_config(settings)?;
    fono_wake::validate_config(&config)
        .map_err(|error| AppError::Config(format!("Некорректная wake phrase: {error}")))?;
    Ok(CalibrationContext {
        backend: settings.wake_backend,
        model_version: SHERPA_MODEL_VERSION.to_owned(),
        phrase: "рамзи".to_owned(),
        graph: "sherpa-bpe-v1:ramzi".to_owned(),
        threshold: settings.wake_word_threshold,
        sensitivity: settings.wake_word_sensitivity,
        vad_threshold: settings.wake_word_vad_threshold,
    })
}

fn assess_samples(samples: &[i16]) -> WakeCalibrationSampleResult {
    let (rms, peak) = crate::application::wake::normalized_levels(samples);
    let active_samples = samples
        .iter()
        .filter(|sample| (((**sample as i32).abs() as f32) / i16::MAX as f32) >= ACTIVE_SIGNAL)
        .count() as u64;
    let active_ms = active_samples.saturating_mul(1_000) / SAMPLE_RATE;
    let reason = if peak >= CLIPPING_PEAK {
        Some(WakeCalibrationRejection::Clipping)
    } else if rms < SILENCE_RMS {
        Some(WakeCalibrationRejection::Silence)
    } else if active_ms < MIN_ACTIVE_MS {
        Some(WakeCalibrationRejection::TooShort)
    } else {
        None
    };
    WakeCalibrationSampleResult {
        accepted: reason.is_none(),
        reason,
        rms,
        peak,
        active_ms,
    }
}

fn build_profile(
    context: &CalibrationContext,
    accepted: &[AcceptedSample],
    rejected_samples: u8,
) -> WakeCalibrationProfile {
    let total = accepted.len().max(1) as f32;
    WakeCalibrationProfile {
        backend: context.backend,
        model_version: context.model_version.clone(),
        phrase: context.phrase.clone(),
        graph: context.graph.clone(),
        threshold: context.threshold,
        sensitivity: context.sensitivity,
        vad_threshold: context.vad_threshold,
        completed_at: chrono::Utc::now(),
        accepted_samples: accepted.len() as u8,
        rejected_samples,
        average_rms: accepted.iter().map(|sample| sample.rms).sum::<f32>() / total,
        average_peak: accepted.iter().map(|sample| sample.peak).sum::<f32>() / total,
        average_active_ms: accepted.iter().map(|sample| sample.active_ms).sum::<u64>()
            / accepted.len().max(1) as u64,
        validation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_silence_without_retaining_audio() {
        let result = assess_samples(&vec![0; SAMPLE_RATE as usize]);
        assert!(!result.accepted);
        assert_eq!(result.reason, Some(WakeCalibrationRejection::Silence));
    }

    #[test]
    fn rejects_clipping_before_other_quality_checks() {
        let result = assess_samples(&[i16::MAX, 0]);
        assert!(!result.accepted);
        assert_eq!(result.reason, Some(WakeCalibrationRejection::Clipping));
    }

    #[test]
    fn rejects_short_speech_activity() {
        let mut samples = vec![0; SAMPLE_RATE as usize];
        samples[..(SAMPLE_RATE / 10) as usize].fill(2_000);
        let result = assess_samples(&samples);
        assert!(!result.accepted);
        assert_eq!(result.reason, Some(WakeCalibrationRejection::TooShort));
    }

    #[test]
    fn accepts_a_clear_short_phrase() {
        let mut samples = vec![0; SAMPLE_RATE as usize];
        samples[..(SAMPLE_RATE / 2) as usize].fill(2_000);
        let result = assess_samples(&samples);
        assert!(result.accepted);
        assert_eq!(result.reason, None);
        assert_eq!(result.active_ms, 500);
    }

    #[test]
    fn profile_contains_only_aggregated_signal_metrics() {
        let context = CalibrationContext {
            backend: WakeWordBackend::SherpaOnnx,
            model_version: "model-v1".into(),
            phrase: "рамзи".into(),
            graph: "graph-v1".into(),
            threshold: 0.25,
            sensitivity: 0.5,
            vad_threshold: 0.015,
        };
        let profile = build_profile(
            &context,
            &[
                AcceptedSample {
                    rms: 0.1,
                    peak: 0.3,
                    active_ms: 500,
                },
                AcceptedSample {
                    rms: 0.2,
                    peak: 0.4,
                    active_ms: 700,
                },
            ],
            3,
        );

        assert_eq!(profile.accepted_samples, 2);
        assert_eq!(profile.rejected_samples, 3);
        assert!((profile.average_rms - 0.15).abs() < f32::EPSILON);
        assert_eq!(profile.average_active_ms, 600);
        let serialized = serde_json::to_value(&profile).unwrap();
        assert!(serialized.get("samples").is_none());
    }
}
