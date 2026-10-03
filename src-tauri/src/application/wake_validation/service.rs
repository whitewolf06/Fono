use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::state::{self, AppState};
use crate::types::{Settings, WakeCalibrationProfile, WakeCalibrationValidation};

use super::rules::{
    expects_detection, has_current_validation, input_issue, profile_matches_settings,
    requires_profile, WakeProfileValidationKind, WakeProfileValidationSampleResult,
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
    service.bind_profile(
        settings
            .wake_calibration_profile
            .as_ref()
            .expect("validated profile exists"),
    );
    Ok(status(app))
}

pub async fn record(
    app: AppHandle,
    kind: WakeProfileValidationKind,
) -> AppResult<WakeProfileValidationStatus> {
    let service = app.state::<WakeProfileValidationService>();
    let settings = app.state::<AppState>().settings();
    let profile = profile_for_validation(&settings)?;
    if !service.matches_profile(profile) {
        return Err(AppError::Config(
            "Профиль изменился. Начните проверку заново".into(),
        ));
    }
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
    let current = app.state::<AppState>().settings();
    if profile_for_validation(&current).is_err()
        || !current
            .wake_calibration_profile
            .as_ref()
            .is_some_and(|p| service.matches_profile(p))
    {
        service.recording_failed(session_id);
        return Err(AppError::Config(
            "Профиль изменился во время записи. Начните проверку заново".into(),
        ));
    }
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

pub fn cancel(app: &AppHandle) -> AppResult<WakeProfileValidationStatus> {
    app.state::<WakeProfileValidationService>().cancel()?;
    Ok(status(app))
}

/// Blocks live activation only for the personal Sherpa phrase. Existing bundled
/// phrases keep their prior behavior, and a profile must match current settings.
pub fn ensure_profile_can_activate(settings: &Settings) -> AppResult<()> {
    if !requires_profile(settings) {
        return Ok(());
    }
    let profile = settings.wake_calibration_profile.as_ref().ok_or_else(|| {
        AppError::Config("Сначала настройте фразу: пять повторов и свежая проверка".into())
    })?;
    if profile.validation.is_none() {
        return Err(AppError::Config(
            "Фраза ещё не прошла проверку. Завершите три повтора и две отрицательные проверки"
                .into(),
        ));
    }
    if !has_current_validation(settings) {
        return Err(AppError::Config(
            "Настройки фразы изменились после проверки. Пройдите настройку заново".into(),
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
    if !crate::application::wake_calibration::supports_setup(settings.wake_backend) {
        return Err(AppError::Config(
            "Проверка профиля доступна для локальных Sherpa-движков".into(),
        ));
    }
    let profile = settings.wake_calibration_profile.as_ref().ok_or_else(|| {
        AppError::Config("Сначала завершите пять повторов настройки фразы".into())
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
            graph: crate::application::wake_calibration::profile_graph(settings),
            threshold: settings.wake_word_threshold,
            sensitivity: settings.wake_word_sensitivity,
            vad_threshold: settings.wake_word_vad_threshold,
            completed_at: chrono::Utc::now(),
            accepted_samples: 5,
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
