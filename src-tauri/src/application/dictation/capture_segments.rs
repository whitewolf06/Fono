//! Opt-in capture recognition; only Stop publishes the combined result.
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    types::Settings,
};
use tauri::{AppHandle, Manager};

pub(super) fn maybe_start(app: &AppHandle, operation: u64, settings: &Settings) -> AppResult<bool> {
    if !settings.pretranscription_enabled {
        return Ok(false);
    }
    start(app, operation, settings)
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
            stt.transcribe_window_with_options(
                samples,
                &settings.language,
                context,
                operation,
                cancellation,
                start,
                settings.decoding_options(),
            )
        },
    )
}
