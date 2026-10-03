use crate::{
    error::{AppError, AppResult},
    types::{Settings, WakeCalibrationProfile, WakeWordBackend},
};
pub const REQUIRED_SAMPLES: u8 = 5;
pub const SAMPLE_DURATION_MS: u64 = 4000;
pub(super) const CANDIDATES: [f32; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeCalibrationSampleResult {
    pub accepted: bool,
    pub detected: bool,
    pub matched_candidates: u8,
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
    PhraseNotDetected,
}

pub(super) fn assess_samples(samples: &[i16]) -> WakeCalibrationSampleResult {
    let (rms, peak) = crate::application::wake::normalized_levels(samples);
    let active_ms = samples
        .iter()
        .filter(|s| ((**s as i32).abs() as f32) / 32768.0 >= 0.02)
        .count() as u64
        * 1000
        / 16000;
    let reason = if peak >= 0.985 {
        Some(WakeCalibrationRejection::Clipping)
    } else if rms < 0.007 {
        Some(WakeCalibrationRejection::Silence)
    } else if active_ms < 300 {
        Some(WakeCalibrationRejection::TooShort)
    } else {
        None
    };
    WakeCalibrationSampleResult {
        accepted: reason.is_none(),
        detected: false,
        matched_candidates: 0,
        reason,
        rms,
        peak,
        active_ms,
    }
}
pub fn supports_setup(backend: WakeWordBackend) -> bool {
    matches!(
        backend,
        WakeWordBackend::SherpaOnnx
            | WakeWordBackend::SherpaStreamingRu
            | WakeWordBackend::SherpaStreamingEn
    )
}
pub fn profile_graph(settings: &Settings) -> String {
    format!(
        "fono-wake-tuning-v2:{:?}:{}:confirmations=2:device={}:wake-model={:?}",
        settings.wake_backend,
        settings.wake_word.trim().to_lowercase(),
        settings
            .audio_device_id
            .as_deref()
            .unwrap_or("system-default"),
        settings.wake_word_model,
    )
}
pub fn profile_matches(profile: &WakeCalibrationProfile, settings: &Settings) -> bool {
    supports_setup(settings.wake_backend)
        && profile.backend == settings.wake_backend
        && profile.model_version == fono_wake::model_version(settings.wake_backend)
        && profile
            .phrase
            .trim()
            .eq_ignore_ascii_case(settings.wake_word.trim())
        && profile.graph == profile_graph(settings)
        && profile.accepted_samples == REQUIRED_SAMPLES
        && (profile.threshold - settings.wake_word_threshold).abs() < f32::EPSILON
        && (profile.sensitivity - settings.wake_word_sensitivity).abs() < f32::EPSILON
        && (profile.vad_threshold - settings.wake_word_vad_threshold).abs() < f32::EPSILON
}
pub(super) fn validate_setup(settings: &Settings) -> AppResult<()> {
    if settings.wake_word_enabled {
        return Err(AppError::Config(
            "Выключите пробуждение перед настройкой фразы".into(),
        ));
    }
    if !supports_setup(settings.wake_backend) {
        return Err(AppError::Config(
            "Настройка фразы доступна для локальных Sherpa-движков".into(),
        ));
    }
    fono_wake::validate_config(&crate::settings_to_wake_config(settings)?)
        .map_err(|e| AppError::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_silence() {
        assert_eq!(
            assess_samples(&[0; 16000]).reason,
            Some(WakeCalibrationRejection::Silence)
        );
    }
    #[test]
    fn quality_is_not_a_detection() {
        let result = assess_samples(&[2000; 16000]);
        assert!(result.accepted);
        assert!(!result.detected);
    }
}
