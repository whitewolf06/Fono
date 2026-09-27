use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::types::{Settings, WakeCalibrationProfile, WakeCalibrationValidation};

use super::rules::{
    expects_detection, has_current_validation, input_issue, is_ramzi_sherpa,
    profile_matches_settings, WakeProfileValidationKind, WakeProfileValidationSampleResult,
    WakeProfileValidationStatus, SAMPLE_DURATION_MS,
};
use super::state::WakeProfileValidationService;

pub fn start(app: &AppHandle) -> AppResult<WakeProfileValidationStatus> {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    if settings.wake_word_enabled {
        return Err(AppError::Config(
            "Выключите wake word перед проверкой профиля: это исключает конкурирующую запись"
                .into(),
        ));
    }
    if crate::application::wake_calibration::status(app).active {
        return Err(AppError::Config(
            "Сначала завершите или отмените калибровку wake word".into(),
        ));
    }
    let threshold = profile_for_validation(&settings)?.threshold;
    settings
        .wake_calibration_profile
        .as_mut()
        .expect("validated profile exists")
        .validation = None;
    state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    crate::events::emit_settings(app, &settings);
    let service = app.state::<WakeProfileValidationService>();
    service.begin(threshold);
    Ok(status(app))
}

pub async fn record(
    app: AppHandle,
    kind: WakeProfileValidationKind,
) -> AppResult<WakeProfileValidationStatus> {
    let service = app.state::<WakeProfileValidationService>();
    let session_id = service.begin_recording(kind)?;
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
    let result = if let Some(input_issue) = input_issue(kind, &recorded.samples) {
        drop(recorded);
        WakeProfileValidationSampleResult {
            kind,
            detected: false,
            accepted: false,
            input_issue: Some(input_issue),
        }
    } else {
        match crate::application::wake::recognize_transient_samples(&app, recorded.samples).await {
            Ok(report) => WakeProfileValidationSampleResult {
                kind,
                detected: report.detected,
                accepted: report.detected == expects_detection(kind),
                input_issue: None,
            },
            Err(error) => {
                service.recording_failed(session_id);
                return Err(error);
            }
        }
    };
    if let Some(validation) = service.record_result(session_id, result) {
        persist_validation(&app, validation)?;
    }
    Ok(status(&app))
}

pub fn status(app: &AppHandle) -> WakeProfileValidationStatus {
    let settings = app.state::<AppState>().settings();
    app.state::<WakeProfileValidationService>()
        .status(has_current_validation(&settings))
}

/// Blocks live activation only for the personal Sherpa phrase. Existing bundled
/// phrases keep their prior behavior, and a profile must match current settings.
pub fn ensure_profile_can_activate(settings: &Settings) -> AppResult<()> {
    if !is_ramzi_sherpa(settings) {
        return Ok(());
    }
    let profile = settings.wake_calibration_profile.as_ref().ok_or_else(|| {
        AppError::Config("Сначала пройдите калибровку и проверку профиля «рамзи»".into())
    })?;
    if profile.validation.is_none() {
        return Err(AppError::Config(
            "Профиль «рамзи» ещё не прошёл проверку. Пока используйте hotkey".into(),
        ));
    }
    if !has_current_validation(settings) {
        return Err(AppError::Config(
            "Настройки «рамзи» изменились после проверки. Пройдите проверку заново".into(),
        ));
    }
    Ok(())
}

fn persist_validation(app: &AppHandle, validation: WakeCalibrationValidation) -> AppResult<()> {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    let profile = settings.wake_calibration_profile.as_mut().ok_or_else(|| {
        AppError::Config("Профиль калибровки был удалён во время проверки".into())
    })?;
    profile.validation = Some(validation);
    state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    crate::events::emit_settings(app, &settings);
    Ok(())
}

fn profile_for_validation(settings: &Settings) -> AppResult<&WakeCalibrationProfile> {
    if !is_ramzi_sherpa(settings) {
        return Err(AppError::Config(
            "Проверка профиля доступна для Sherpa-фразы «рамзи»".into(),
        ));
    }
    let profile = settings.wake_calibration_profile.as_ref().ok_or_else(|| {
        AppError::Config("Сначала завершите 10 образцов калибровки «рамзи»".into())
    })?;
    if !profile_matches_settings(profile, settings) {
        return Err(AppError::Config(
            "Профиль не соответствует текущим настройкам. Пройдите калибровку заново".into(),
        ));
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::super::rules::{NEGATIVE_REQUIRED, POSITIVE_REQUIRED, SHERPA_MODEL_VERSION};
    use super::*;
    use crate::types::WakeWordBackend;

    fn sample_profile(
        settings: &Settings,
        validation: Option<WakeCalibrationValidation>,
    ) -> WakeCalibrationProfile {
        WakeCalibrationProfile {
            backend: WakeWordBackend::SherpaOnnx,
            model_version: SHERPA_MODEL_VERSION.into(),
            phrase: "рамзи".into(),
            graph: "sherpa-bpe-v1:ramzi".into(),
            threshold: settings.wake_word_threshold,
            sensitivity: settings.wake_word_sensitivity,
            vad_threshold: settings.wake_word_vad_threshold,
            completed_at: chrono::Utc::now(),
            accepted_samples: 10,
            rejected_samples: 0,
            average_rms: 0.1,
            average_peak: 0.4,
            average_active_ms: 500,
            validation,
        }
    }

    #[test]
    fn ramzi_listener_requires_a_current_successful_validation() {
        let mut settings = Settings::default();
        settings.wake_word = "рамзи".into();
        assert!(ensure_profile_can_activate(&settings).is_err());

        let validation = WakeCalibrationValidation {
            completed_at: chrono::Utc::now(),
            positive_passed: POSITIVE_REQUIRED,
            positive_required: POSITIVE_REQUIRED,
            negative_passed: NEGATIVE_REQUIRED,
            negative_required: NEGATIVE_REQUIRED,
            confirmed_threshold: settings.wake_word_threshold,
        };
        settings.wake_calibration_profile = Some(sample_profile(&settings, Some(validation)));
        assert!(ensure_profile_can_activate(&settings).is_ok());

        settings.wake_word_threshold = 0.3;
        assert!(ensure_profile_can_activate(&settings).is_err());
    }
}
