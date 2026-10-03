//! Five actual detector tuning samples, followed by separate fresh validation.
mod rules;
mod state;
use crate::{
    error::{AppError, AppResult},
    state::{self as app_state, AppState},
    types::WakeCalibrationProfile,
};
pub use rules::{
    profile_graph, profile_matches, supports_setup, WakeCalibrationRejection,
    WakeCalibrationSampleResult, REQUIRED_SAMPLES, SAMPLE_DURATION_MS,
};
pub use state::{WakeCalibrationService, WakeCalibrationStatus};
use tauri::{AppHandle, Manager};

pub fn start(app: &AppHandle) -> AppResult<WakeCalibrationStatus> {
    if crate::application::wake_validation::status(app).active {
        return Err(AppError::Config("Сначала завершите проверку фразы".into()));
    }
    let settings = app.state::<AppState>().settings();
    rules::validate_setup(&settings)?;
    if !crate::application::models::is_kws_model_downloaded(app)? {
        return Err(AppError::Config(
            "Сначала загрузите модель выбранного языка пробуждения".into(),
        ));
    }
    app.state::<WakeCalibrationService>().begin(settings)?;
    Ok(status(app))
}
pub async fn record_next(app: AppHandle) -> AppResult<WakeCalibrationStatus> {
    let service = app.state::<WakeCalibrationService>();
    let (id, settings) = service.begin_recording()?;
    let recorded =
        match crate::application::wake::record_transient_sample(app.clone(), SAMPLE_DURATION_MS)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                service.failed(id);
                return Err(e);
            }
        };
    let mut result = rules::assess_samples(&recorded.samples);
    let mut hits = [false; 5];
    if result.accepted {
        let mut config = match crate::settings_to_wake_config(&settings) {
            Ok(config) => config,
            Err(error) => {
                service.failed(id);
                return Err(error);
            }
        };
        let samples = recorded.samples;
        let tuned = tauri::async_runtime::spawn_blocking(move || {
            #[cfg(feature = "sherpa-wake")]
            {
                let mut hits = [false; 5];
                for (index, threshold) in rules::CANDIDATES.iter().enumerate() {
                    config.threshold = *threshold;
                    config.phrase_stability_ms = 150 + (*threshold * 700.0) as u64;
                    let report = fono_wake::replay::replay_samples(&config, &samples)?;
                    hits[index] = report.detections > 0;
                }
                Ok::<_, fono_wake::WakeWordError>(hits)
            }
            #[cfg(not(feature = "sherpa-wake"))]
            {
                let _ = (config, samples);
                Err::<[bool; 5], _>(fono_wake::WakeWordError::BackendNotCompiled(
                    fono_wake::WakeWordBackend::SherpaOnnx,
                ))
            }
        })
        .await;
        hits = match tuned {
            Ok(Ok(h)) => h,
            Ok(Err(error)) => {
                service.failed(id);
                return Err(error.into());
            }
            Err(error) => {
                service.failed(id);
                return Err(AppError::Internal(format!("wake tuning failed: {error}")));
            }
        };
        result.detected = hits.iter().any(|hit| *hit);
        result.matched_candidates = hits.iter().filter(|hit| **hit).count() as u8;
        if !result.detected {
            result.accepted = false;
            result.reason = Some(WakeCalibrationRejection::PhraseNotDetected);
        }
    }
    if let Some(profile) = service.record(id, result, hits) {
        if let Err(error) = persist_profile(&app, profile) {
            service.retry_final_sample(id);
            return Err(error);
        }
    }
    Ok(status(&app))
}
pub fn cancel(app: &AppHandle) -> AppResult<WakeCalibrationStatus> {
    app.state::<WakeCalibrationService>().cancel()?;
    crate::application::wake_validation::cancel(app)?;
    Ok(status(app))
}
pub fn status(app: &AppHandle) -> WakeCalibrationStatus {
    let settings = app.state::<AppState>().settings();
    let mut profile = settings
        .wake_calibration_profile
        .clone()
        .filter(|p| profile_matches(p, &settings));
    if let Some(profile) = profile.as_mut() {
        if profile.validation.as_ref().is_some_and(|v| {
            v.positive_passed < 3
                || v.negative_passed < 2
                || (v.confirmed_threshold - settings.wake_word_threshold).abs() >= f32::EPSILON
        }) {
            profile.validation = None;
        }
    }
    app.state::<WakeCalibrationService>().status(profile)
}
fn persist_profile(app: &AppHandle, profile: WakeCalibrationProfile) -> AppResult<()> {
    let state = app.state::<AppState>();
    let mut settings = state.settings();
    if profile.graph != profile_graph(&settings)
        || (profile.sensitivity - settings.wake_word_sensitivity).abs() >= f32::EPSILON
        || (profile.vad_threshold - settings.wake_word_vad_threshold).abs() >= f32::EPSILON
        || settings.wake_word_enabled
    {
        return Err(AppError::Config(
            "Фраза изменилась во время настройки. Повторите настройку".into(),
        ));
    }
    settings.wake_word_threshold = profile.threshold;
    settings.wake_calibration_profile = Some(profile);
    app_state::save_settings(&settings)?;
    state.set_settings(settings.clone());
    crate::events::emit_settings(app, &settings);
    Ok(())
}
