use crate::types::{Settings, WakeCalibrationProfile, WakeWordBackend};

use super::state::ValidationSession;

pub(super) const POSITIVE_REQUIRED: u8 = 3;
pub(super) const NEGATIVE_REQUIRED: u8 = 2;
pub(super) const SAMPLE_DURATION_MS: u64 = 3_000;
const SAMPLE_RATE: u64 = 16_000;
const SILENCE_RMS: f32 = 0.007;
const CLIPPING_PEAK: f32 = 0.985;
const ACTIVE_SIGNAL: f32 = 0.02;
const MIN_ACTIVE_MS: u64 = 300;
#[cfg(test)]
pub(super) const SHERPA_MODEL_VERSION: &str =
    "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01";

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeProfileValidationStatus {
    pub active: bool,
    pub recording: bool,
    pub failed: bool,
    pub completed: bool,
    pub positive_passed: u8,
    pub positive_required: u8,
    pub silence_passed: bool,
    pub other_phrase_passed: bool,
    pub negative_required: u8,
    pub latest_result: Option<WakeProfileValidationSampleResult>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeProfileValidationSampleResult {
    pub kind: WakeProfileValidationKind,
    pub detected: bool,
    pub accepted: bool,
    pub input_issue: Option<WakeProfileValidationInputIssue>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeProfileValidationKind {
    Positive,
    Silence,
    OtherPhrase,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeProfileValidationInputIssue {
    Silence,
    UnexpectedSpeech,
    Clipping,
    TooShort,
}

pub(super) fn kind_complete(session: &ValidationSession, kind: WakeProfileValidationKind) -> bool {
    match kind {
        WakeProfileValidationKind::Positive => session.positive_passed >= POSITIVE_REQUIRED,
        WakeProfileValidationKind::Silence => session.silence_passed,
        WakeProfileValidationKind::OtherPhrase => session.other_phrase_passed,
    }
}

pub(super) fn expects_detection(kind: WakeProfileValidationKind) -> bool {
    matches!(kind, WakeProfileValidationKind::Positive)
}

pub(super) fn input_issue(
    kind: WakeProfileValidationKind,
    samples: &[i16],
) -> Option<WakeProfileValidationInputIssue> {
    let (rms, peak) = crate::application::wake::normalized_levels(samples);
    if kind == WakeProfileValidationKind::Silence {
        if peak >= CLIPPING_PEAK {
            return Some(WakeProfileValidationInputIssue::Clipping);
        }
        return (rms >= SILENCE_RMS).then_some(WakeProfileValidationInputIssue::UnexpectedSpeech);
    }
    if peak >= CLIPPING_PEAK {
        return Some(WakeProfileValidationInputIssue::Clipping);
    }
    if rms < SILENCE_RMS {
        return Some(WakeProfileValidationInputIssue::Silence);
    }
    let active_ms = samples
        .iter()
        .filter(|sample| (((**sample as i32).abs() as f32) / i16::MAX as f32) >= ACTIVE_SIGNAL)
        .count() as u64
        * 1_000
        / SAMPLE_RATE;
    (active_ms < MIN_ACTIVE_MS).then_some(WakeProfileValidationInputIssue::TooShort)
}

pub(super) fn requires_profile(settings: &Settings) -> bool {
    matches!(
        settings.wake_backend,
        WakeWordBackend::SherpaStreamingRu | WakeWordBackend::SherpaStreamingEn
    ) || (settings.wake_backend == WakeWordBackend::SherpaOnnx
        && settings.wake_word.trim().eq_ignore_ascii_case("рамзи"))
}

pub(super) fn profile_matches_settings(
    profile: &WakeCalibrationProfile,
    settings: &Settings,
) -> bool {
    crate::application::wake_calibration::profile_matches(profile, settings)
}
pub(super) fn has_current_validation(settings: &Settings) -> bool {
    let Some(profile) = settings.wake_calibration_profile.as_ref() else {
        return false;
    };
    let Some(validation) = profile.validation.as_ref() else {
        return false;
    };
    profile_matches_settings(profile, settings)
        && validation.positive_passed >= POSITIVE_REQUIRED
        && validation.negative_passed >= NEGATIVE_REQUIRED
        && (validation.confirmed_threshold - settings.wake_word_threshold).abs() < f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_and_other_phrase_require_audible_speech() {
        let silent = vec![0; SAMPLE_RATE as usize];
        assert_eq!(
            input_issue(WakeProfileValidationKind::Positive, &silent),
            Some(WakeProfileValidationInputIssue::Silence)
        );
        assert_eq!(
            input_issue(WakeProfileValidationKind::OtherPhrase, &silent),
            Some(WakeProfileValidationInputIssue::Silence)
        );
        assert_eq!(
            input_issue(WakeProfileValidationKind::Silence, &silent),
            None
        );
        let spoken = vec![2_000; SAMPLE_RATE as usize];
        assert_eq!(
            input_issue(WakeProfileValidationKind::Silence, &spoken),
            Some(WakeProfileValidationInputIssue::UnexpectedSpeech)
        );
    }

    #[test]
    fn profile_matching_rejects_changed_sensitivity() {
        let mut settings = Settings::default();
        settings.wake_word = "рамзи".into();
        let profile = WakeCalibrationProfile {
            backend: WakeWordBackend::SherpaOnnx,
            model_version: SHERPA_MODEL_VERSION.into(),
            phrase: "рамзи".into(),
            graph: crate::application::wake_calibration::profile_graph(&settings),
            threshold: settings.wake_word_threshold,
            sensitivity: settings.wake_word_sensitivity,
            vad_threshold: settings.wake_word_vad_threshold,
            completed_at: chrono::Utc::now(),
            accepted_samples: 5,
            rejected_samples: 0,
            average_rms: 0.1,
            average_peak: 0.4,
            average_active_ms: 500,
            validation: None,
        };
        assert!(profile_matches_settings(&profile, &settings));
        settings.wake_word_sensitivity = 0.4;
        assert!(!profile_matches_settings(&profile, &settings));
    }
}
