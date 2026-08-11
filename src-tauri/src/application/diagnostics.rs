//! Application use cases for local logs and microphone diagnostics.

use tauri::{AppHandle, Manager};

use std::io::{Read, Seek, SeekFrom};

use crate::error::{AppError, AppResult};
use crate::operation::{OperationSource, TerminalReason};
use crate::pipeline::{self, Pipeline};
use crate::state::{self, AppState};
use crate::types::PipelineState;

pub fn clear_logs() -> AppResult<()> {
    let log_dir = state::app_data_dir()?.join("logs");
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(AppError::Io)?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    let mut matching: Vec<_> = entries
        .drain(..)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("fono.log"))
        .collect();
    matching.sort_by_key(|entry| entry.file_name());
    if let Some(active) = matching.pop() {
        for archived in matching {
            std::fs::remove_file(archived.path()).map_err(AppError::Io)?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(active.path())
            .map_err(AppError::Io)?;
        tracing::info!("log files cleared");
    }
    Ok(())
}

pub fn get_recent_logs(lines: Option<usize>) -> AppResult<String> {
    let limit = lines.unwrap_or(80).min(500);
    let log_dir = state::app_data_dir()?.join("logs");
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(AppError::Io)?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    let target = entries
        .iter()
        .rev()
        .find(|entry| entry.file_name().to_string_lossy().starts_with("fono.log"))
        .ok_or_else(|| AppError::Internal("лог-файл не найден".into()))?;

    const MAX_LOG_TAIL_BYTES: u64 = 1024 * 1024;
    let mut file = std::fs::File::open(target.path())?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(MAX_LOG_TAIL_BYTES)))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    if length > MAX_LOG_TAIL_BYTES {
        if let Some(first_newline) = content.find('\n') {
            content.drain(..=first_newline);
        }
    }
    Ok(content
        .lines()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

pub async fn test_microphone(app: AppHandle, duration_ms: u64) -> AppResult<MicTestResult> {
    let state = app.state::<AppState>();
    let pipeline = app.state::<Pipeline>();
    let settings = state.settings();

    if let Err(error) = pipeline.start_recording_from(
        settings.audio_device_id.as_deref(),
        OperationSource::Diagnostics,
    ) {
        emit_pipeline_error(&app, &error.to_string());
        tracing::error!("test_microphone: start_recording FAILED: {error}");
        return Err(error);
    }
    pipeline::set_state(&app, state.inner(), PipelineState::Listening);
    let operation = pipeline.operation_id();
    let cancellation = pipeline.cancellation(operation).ok_or_else(|| {
        AppError::Internal("active microphone test has no cancellation signal".into())
    })?;

    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_millis(
            duration_ms.clamp(500, 5_000),
        )) => {}
        _ = crate::application::dictation::wait_for_cancellation(cancellation) => {
            return Err(AppError::Cancelled("microphone test cancelled".into()));
        }
    }

    let samples = match pipeline.stop_recording() {
        Ok(samples) => samples,
        Err(error) => {
            emit_pipeline_error(&app, &error.to_string());
            tracing::error!("test_microphone: stop_recording FAILED: {error}");
            let _ = pipeline::set_state_for_operation(
                &app,
                state.inner(),
                &pipeline,
                operation,
                PipelineState::Idle,
                TerminalReason::Failed,
            );
            return Err(error);
        }
    };
    let _ = pipeline::set_state_for_operation(
        &app,
        state.inner(),
        &pipeline,
        operation,
        PipelineState::Idle,
        TerminalReason::Completed,
    );

    if samples.is_empty() {
        emit_pipeline_error(&app, "No input captured. Check microphone and permissions.");
        return Err(AppError::Audio(
            "не получено ни одного сэмпла — микрофон молчит или занят".into(),
        ));
    }

    let mut peak: i32 = 0;
    let mut sum_sq: i64 = 0;
    for &sample in &samples {
        let magnitude = sample.unsigned_abs() as i32;
        peak = peak.max(magnitude);
        sum_sq += (sample as i64) * (sample as i64);
    }
    let rms = ((sum_sq as f64 / samples.len() as f64).sqrt()) as f32;

    Ok(MicTestResult {
        samples: samples.len(),
        duration_ms: (samples.len() as f64 / 16_000.0 * 1000.0) as u64,
        peak: peak as f32 / i16::MAX as f32,
        rms: rms / i16::MAX as f32,
    })
}

fn emit_pipeline_error(app: &AppHandle, message: &str) {
    crate::events::emit_error(
        app,
        crate::events::ErrorCodeV1::Audio,
        message,
        app.state::<Pipeline>()
            .current_operation()
            .map(|item| item.id),
    );
}

#[derive(Debug, serde::Serialize)]
pub struct MicTestResult {
    pub samples: usize,
    pub duration_ms: u64,
    pub peak: f32,
    pub rms: f32,
}
