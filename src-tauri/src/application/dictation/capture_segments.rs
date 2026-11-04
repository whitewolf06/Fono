//! Opt-in capture recognition; only Stop publishes the combined result.
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    types::Settings,
};
use tauri::{AppHandle, Manager};

pub(super) fn maybe_start(app: &AppHandle, operation: u64, settings: &Settings) -> AppResult<bool> {
    if !capture_pretranscription_enabled(settings) {
        return Ok(false);
    }
    start(app, operation, settings)
}

fn capture_pretranscription_enabled(settings: &Settings) -> bool {
    // Remote capture pretranscription has a separate rollout. Keeping this
    // preference must never start a dormant local model in server mode.
    settings.stt_source == crate::types::SttSource::Local && settings.pretranscription_enabled
}

fn start(app: &AppHandle, operation: u64, settings: &Settings) -> AppResult<bool> {
    let model = settings
        .selected_stt_model()
        .ok_or(AppError::ModelNotSelected)?;
    let pipeline = app.state::<Pipeline>();
    let stt = pipeline.stt().clone();
    let workers = crate::stt::worker_paths_for_app(app);
    let vad_path = super::super::speech_gate::model_path(app)?;
    let settings = settings.clone();
    let glossary =
        crate::application::personal_dictionary::GlossarySnapshot::from_settings(&settings);
    pipeline.start_capture_segments_for(
        operation,
        vad_path,
        crate::pipeline::pretranscription::START_AFTER_SAMPLES,
        move |samples, start, context, cancellation| {
            stt.ensure_model_loaded(&model, settings.acceleration, &workers, cancellation)?;
            let context = context.filter(|_| {
                stt.capabilities()
                    .is_some_and(|capabilities| capabilities.initial_prompt)
            });
            stt.transcribe_window_with_glossary(
                samples,
                &settings.language,
                context,
                operation,
                cancellation,
                start,
                settings.decoding_options(),
                glossary.terms(),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn remote_source_does_not_admit_capture_pretranscription() {
        let local = crate::types::Settings {
            pretranscription_enabled: true,
            ..Default::default()
        };
        let remote = crate::types::Settings {
            stt_source: crate::types::SttSource::FonoServer,
            ..local.clone()
        };
        assert!(!super::capture_pretranscription_enabled(&remote));
        assert!(super::capture_pretranscription_enabled(&local));
    }
}
