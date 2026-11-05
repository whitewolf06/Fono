//! File jobs use the server's batch queue, independently of the interactive
//! recognizer. The caller supplies already decoded mono PCM, not a local path.
mod wire;

use std::{
    fmt,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fono_core::OperationCancellation;
use fono_types::{ModelId, SttEngine, TranscriptionResult};

use super::{failure, transport::Transport, RemoteEndpoint, RemoteError, RemoteErrorKind};
use crate::{error::AppResult, recognizer::check_cancellation, ActivityGuard};

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MAX_AUDIO_SECONDS: u64 = 3 * 60 * 60;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const WAV_HEADER_BYTES: usize = 44;

/// One immutable endpoint/model snapshot for a file job. Endpoint credentials,
/// user filenames and server messages cannot enter its Debug representation.
pub struct RemoteTranscriptionClient {
    transport: Arc<Transport>,
    activity: Arc<dyn ActivityGuard>,
    wire_model: String,
    languages: Vec<String>,
    pub(super) max_samples: usize,
}

struct JobRequest<'a> {
    samples: usize,
    language: &'a str,
    started: Instant,
    deadline: Duration,
}

impl fmt::Debug for RemoteTranscriptionClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteTranscriptionClient")
            .finish_non_exhaustive()
    }
}

impl RemoteTranscriptionClient {
    pub fn connect(
        endpoint: RemoteEndpoint,
        model: ModelId,
        activity: Arc<dyn ActivityGuard>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Self> {
        check_cancellation(cancellation)?;
        if model.variant.is_empty()
            || model.variant.len() > 128
            || !model
                .variant
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        let wire_model = match model.engine {
            SttEngine::Whisper => format!("whisper:{}", model.variant),
            _ => model.public_id(),
        };
        // Multipart text fields are bounded by the server's existing API.
        if wire_model.len() > 64 {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        let transport = Arc::new(Transport::new(endpoint, activity.clone())?);
        let http = transport.clone();
        let (capabilities, models) =
            transport.call(cancellation, transport.discovery_deadline(), || {
                Ok(async move {
                    let capabilities = http
                        .json::<super::wire::Capabilities>(http.get("v1/capabilities"), 64 * 1024)
                        .await?;
                    let models = http
                        .json::<super::wire::Models>(http.get("v1/models"), 512 * 1024)
                        .await?;
                    Ok((capabilities, models))
                })
            })?;
        if capabilities.product != "fono-server"
            || capabilities.api_version < 2
            || !capabilities.features.jobs
            || !capabilities
                .audio_formats
                .iter()
                .any(|format| format == "audio/wav")
            || capabilities.max_audio_seconds == 0
            || capabilities.max_upload_bytes <= WAV_HEADER_BYTES as u64
            || models.object != "list"
        {
            return failure(RemoteErrorKind::Incompatible);
        }
        let mut matches = models
            .data
            .into_iter()
            .filter(|model| model.id == wire_model);
        let Some(selected) = matches.next() else {
            return failure(RemoteErrorKind::ModelNotFound);
        };
        if matches.next().is_some()
            || selected.object != "model"
            || selected.fono.engine != model.engine
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        if !matches!(
            selected.fono.disk_state.as_str(),
            "verified" | "present_unverified"
        ) || selected.fono.state == "failed"
        {
            return failure(RemoteErrorKind::ModelNotReady);
        }
        let upload_bytes = capabilities.max_upload_bytes.min(MAX_UPLOAD_BYTES as u64);
        let max_samples = capabilities
            .max_audio_seconds
            .min(MAX_AUDIO_SECONDS)
            .saturating_mul(16_000)
            .min((upload_bytes - WAV_HEADER_BYTES as u64) / 2) as usize;
        Ok(Self {
            transport,
            activity,
            wire_model,
            languages: selected.fono.languages,
            max_samples,
        })
    }

    /// Upload a normalized WAV, wait for its queued server job, and return
    /// only validated results. Call on the host's blocking executor.
    ///
    /// Once the ID is known, cancellation and errors attempt bounded cleanup.
    /// Submission drains its bounded response even after caller cancellation
    /// so an accepted ID can still be cancelled. If that response is lost,
    /// the current server protocol cannot identify the accepted job for cleanup.
    pub fn transcribe_pcm_job(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> AppResult<TranscriptionResult> {
        check_cancellation(cancellation)?;
        if samples.is_empty()
            || samples.len() > self.max_samples
            || language.is_empty()
            || language.len() > 16
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            || !self.languages.iter().any(|supported| supported == language)
        {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        // Retain admission across poll intervals, upload response draining and
        // cleanup; no update/source teardown may race the actual HTTP work.
        let _activity = self.activity.lease()?;
        let started = Instant::now();
        let deadline = self.transport.deadline(samples.len());
        let http = self.transport.clone();
        let model = self.wire_model.clone();
        let language_owned = language.to_owned();
        let submitted = self.transport.call(
            &OperationCancellation::default(),
            deadline.min(Duration::from_secs(30)),
            || {
                check_cancellation(cancellation)?;
                let boundary = format!("fono-{}", uuid::Uuid::new_v4().simple());
                let body = multipart(samples, &language_owned, &model, &boundary);
                check_cancellation(cancellation)?;
                let request = http
                    .post("v1/transcriptions")
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(body);
                Ok(async move {
                    http.accepted_json::<wire::Job>(request, MAX_RESPONSE_BYTES)
                        .await
                })
            },
        )?;
        // Never construct a route from an unchecked server ID.
        if !wire::valid_id(&submitted.id) {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        let id = submitted.id.clone();
        let request = JobRequest {
            samples: samples.len(),
            language,
            started,
            deadline,
        };
        let outcome = self.wait_job(submitted, request, cancellation, progress);
        if outcome.is_err() {
            self.cancel_known_job(&id);
        }
        // A late cancel never publishes a previously completed result.
        check_cancellation(cancellation)?;
        outcome
    }

    fn wait_job(
        &self,
        mut job: wire::Job,
        request: JobRequest<'_>,
        cancellation: &OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> AppResult<TranscriptionResult> {
        let id = job.id.clone();
        let mut reported = 0.0;
        loop {
            check_cancellation(cancellation)?;
            if request
                .deadline
                .saturating_sub(request.started.elapsed())
                .is_zero()
            {
                return failure(RemoteErrorKind::Timeout);
            }
            job.validate(Some(&id), request.language)?;
            match job.state.as_str() {
                "completed" => {
                    let result = wire::result(
                        job.result
                            .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?,
                        &self.wire_model,
                        request.samples,
                    )?;
                    check_cancellation(cancellation)?;
                    progress(1.0);
                    return Ok(result);
                }
                "failed" => {
                    return failure(match job.error.as_ref().map(|error| error.code.as_str()) {
                        Some("busy") => RemoteErrorKind::Busy,
                        Some("model_not_found") => RemoteErrorKind::ModelNotFound,
                        Some("model_not_ready") => RemoteErrorKind::ModelNotReady,
                        Some("invalid_request") => RemoteErrorKind::InvalidRequest,
                        Some("worker_failed") => RemoteErrorKind::WorkerFailed,
                        _ => RemoteErrorKind::Internal,
                    })
                }
                "cancelled" => {
                    return Err(crate::error::AppError::Cancelled(
                        "remote transcription job cancelled".into(),
                    ))
                }
                _ => {}
            }
            if let Some(fraction) = job.progress {
                let fraction = fraction.min(0.99);
                if fraction > reported {
                    progress(fraction);
                    reported = fraction;
                }
            }
            let remaining = request.deadline.saturating_sub(request.started.elapsed());
            if remaining.is_zero() {
                return failure(RemoteErrorKind::Timeout);
            }
            let poll_started = Instant::now();
            while poll_started.elapsed() < POLL_INTERVAL.min(remaining) {
                check_cancellation(cancellation)?;
                thread::sleep(Duration::from_millis(10));
            }
            let remaining = request.deadline.saturating_sub(request.started.elapsed());
            if remaining.is_zero() {
                return failure(RemoteErrorKind::Timeout);
            }
            let http = self.transport.clone();
            let path = format!("v1/transcription-jobs/{id}");
            job = self.transport.call(
                cancellation,
                remaining.min(self.transport.discovery_deadline()),
                || {
                    Ok(async move {
                        http.json::<wire::Job>(http.get(&path), MAX_RESPONSE_BYTES)
                            .await
                    })
                },
            )?;
        }
    }

    fn cancel_known_job(&self, id: &str) {
        let http = self.transport.clone();
        let path = format!("v1/transcription-jobs/{id}/cancel");
        // Best effort: already retired/completed jobs can legitimately return
        // 404. A cleanup error never replaces the caller's original outcome.
        let _ = self.transport.cleanup_call(CLEANUP_TIMEOUT, || {
            Ok(async move {
                http.json::<serde_json::Value>(http.post(&path), MAX_RESPONSE_BYTES)
                    .await
            })
        });
    }
}

fn multipart(samples: &[i16], language: &str, model: &str, boundary: &str) -> Vec<u8> {
    let prefix = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\n{language}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n");
    let suffix = format!("\r\n--{boundary}--\r\n");
    let bytes = (samples.len() * 2) as u32;
    let mut body =
        Vec::with_capacity(prefix.len() + WAV_HEADER_BYTES + bytes as usize + suffix.len());
    body.extend_from_slice(prefix.as_bytes());
    body.extend_from_slice(b"RIFF");
    body.extend_from_slice(&(bytes + 36).to_le_bytes());
    body.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    body.extend_from_slice(&16_000_u32.to_le_bytes());
    body.extend_from_slice(&32_000_u32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&16_u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&bytes.to_le_bytes());
    body.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
    body.extend_from_slice(suffix.as_bytes());
    body
}
