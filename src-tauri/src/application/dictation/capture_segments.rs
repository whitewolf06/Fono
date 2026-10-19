//! Native adapter for F-02's capture consumer. Enabling the user flow follows
//! Stop aggregation (FONO-122) and the setting/quality decision (FONO-123).
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    types::Settings,
};
use tauri::{AppHandle, Manager};

const CAPTURE_STT_AVAILABLE: bool = false;
const START_AFTER_SAMPLES: usize = 15 * 16_000;

pub(super) fn maybe_start(app: &AppHandle, operation: u64, settings: &Settings) -> AppResult<bool> {
    if !CAPTURE_STT_AVAILABLE {
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
        START_AFTER_SAMPLES,
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
