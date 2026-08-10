//! IPC commands for local logs and microphone diagnostics.

use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::operation::OperationSource;
use crate::pipeline::{self, Pipeline};
use crate::state::{self, AppState};
use crate::types::PipelineState;

#[tauri::command]
pub fn clear_logs() -> AppResult<()> {
    let log_dir = state::app_data_dir()?.join("logs");
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)
        .map_err(AppError::Io)?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    if let Some(target) = entries
        .iter()
        .rev()
        .find(|entry| entry.file_name().to_string_lossy().starts_with("fono.log"))
    {
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(target.path())
            .map_err(AppError::Io)?;
        tracing::info!("log file cleared");
    }
    Ok(())
}

#[tauri::command]
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

    let content = std::fs::read_to_string(target.path())?;
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

#[tauri::command]
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

    tokio::time::sleep(std::time::Duration::from_millis(
        duration_ms.clamp(500, 5_000),
    ))
    .await;

    let samples = match pipeline.stop_recording() {
        Ok(samples) => samples,
        Err(error) => {
            emit_pipeline_error(&app, &error.to_string());
            tracing::error!("test_microphone: stop_recording FAILED: {error}");
            pipeline::set_state(&app, state.inner(), PipelineState::Idle);
            return Err(error);
        }
    };
    pipeline::set_state(&app, state.inner(), PipelineState::Idle);

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
    let _ = app.emit("error", message);
}

#[derive(Debug, serde::Serialize)]
pub struct MicTestResult {
    pub samples: usize,
    pub duration_ms: u64,
    pub peak: f32,
    pub rms: f32,
}
