use fono_types::{TranscriptionResult, TRANSCRIPTION_PROTOCOL_VERSION};
use serde::Deserialize;

use super::super::{failure, RemoteErrorKind};
use crate::error::AppResult;

#[derive(Deserialize)]
pub(super) struct Job {
    pub id: String,
    pub state: String,
    #[serde(default)]
    pub requested_language: Option<String>,
    #[serde(default)]
    pub progress: Option<f32>,
    // Parse the terminal result only after retaining the validated job ID.
    // A malformed result must not prevent cleanup of that known job.
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<Failure>,
}

#[derive(Deserialize)]
pub(super) struct Failure {
    pub code: String,
}

impl Job {
    pub fn validate(&self, expected_id: Option<&str>, language: &str) -> AppResult<()> {
        if !valid_id(&self.id)
            || expected_id.is_some_and(|id| id != self.id)
            || self
                .requested_language
                .as_ref()
                .is_some_and(|value| value != language)
            || self
                .progress
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || !matches!(
                self.state.as_str(),
                "queued" | "preparing" | "transcribing" | "completed" | "failed" | "cancelled"
            )
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        Ok(())
    }
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn result(
    value: serde_json::Value,
    model: &str,
    samples: usize,
) -> AppResult<TranscriptionResult> {
    let mut result: TranscriptionResult = serde_json::from_value(value)
        .map_err(|_| super::super::RemoteError::new(RemoteErrorKind::InvalidResponse))?;
    let duration = samples as f64 / 16_000.0;
    let duration_ms = (samples as u64).div_ceil(16);
    if result.protocol_version != TRANSCRIPTION_PROTOCOL_VERSION
        || result.model != model
        || result.audio_seconds.is_some_and(|value| {
            !value.is_finite() || value < 0.0 || (f64::from(value) - duration).abs() > 0.01
        })
        || result
            .transcribe_seconds
            .is_some_and(|value| !value.is_finite() || !(0.0..=86_400.0).contains(&value))
        || result.detected_language.as_ref().is_some_and(|value| {
            value.is_empty()
                || value.len() > 16
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        })
    {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    // The v1 job result reports a physical backend, sometimes using the
    // worker's uppercase spelling. Do not substitute a discovery device or
    // invent a CUDA ordinal that is absent from this particular response.
    result.backend = result
        .backend
        .as_deref()
        .map(|backend| {
            super::super::wire::device(match backend {
                "CPU" => "cpu",
                "CUDA" => "cuda",
                "Vulkan" => "vulkan",
                backend => backend,
            })
        })
        .transpose()?;
    let mut previous_start = 0;
    for segment in &result.segments {
        if segment.start_ms < previous_start
            || segment.start_ms > segment.end_ms
            || segment.end_ms > duration_ms
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        previous_start = segment.start_ms;
    }
    Ok(result)
}
