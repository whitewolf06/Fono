//! Native Fono Server API v2 client. Blocking entry points belong on the host's
//! blocking executor, just like the other recognizers; no Tauri or settings I/O.
mod health;
mod inspection;
mod transport;
mod wire;
pub use health::{
    probe_health, RemoteHealth, RemoteModelHealth, RemoteServerHealth, REMOTE_HEALTH_TIMEOUT,
};
pub use inspection::{
    inspect_server, RemoteDevice, RemoteInspection, RemoteInspectionState, RemoteModel,
};

use std::{borrow::Cow, fmt, sync::Arc, time::Instant};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fono_core::OperationCancellation;
use fono_stt_protocol::{BackendKind, DecodingStrategy, TimedSegment, WindowTranscript};
use fono_types::{
    CoreError, ErrorCode, ModelId, RemoteBackend, RemoteTranscriptMetadata,
    SttEngine as ModelEngine, Transcript,
};

use crate::{
    error::AppResult,
    recognizer::{self, Recognizer, RecognizerLoad},
    ActivityGuard, RecognitionOptions, RecognizerCapabilities, WindowOptions,
};
pub use transport::{RemoteEndpoint, RemoteTimeouts};

const PCM_TYPE: &str = "audio/L16;rate=16000;channels=1";
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;

/// Contains only classified metadata, never a URL, token, server message or
/// reqwest source. Safe to format in host diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Fono Server: {kind:?}")]
pub struct RemoteError {
    pub kind: RemoteErrorKind,
    pub status: Option<u16>,
    pub retry_after_seconds: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteErrorKind {
    InvalidEndpoint,
    InvalidRequest,
    Incompatible,
    Unauthorized,
    Busy,
    ModelNotFound,
    ModelNotReady,
    UnsupportedAudio,
    WorkerFailed,
    Network,
    Timeout,
    InvalidResponse,
    Internal,
}

impl RemoteError {
    fn new(kind: RemoteErrorKind) -> Self {
        Self {
            kind,
            status: None,
            retry_after_seconds: None,
        }
    }

    pub fn core_error(&self) -> CoreError {
        use RemoteErrorKind as K;
        let (code, message, retryable) = match self.kind {
            K::InvalidEndpoint | K::InvalidRequest | K::Incompatible => (
                ErrorCode::InvalidRequest,
                "Invalid Fono Server configuration or request",
                false,
            ),
            K::Unauthorized => (
                ErrorCode::Unauthorized,
                "Fono Server authorization failed",
                false,
            ),
            K::Busy => (ErrorCode::Busy, "Fono Server is busy", true),
            K::ModelNotFound => (
                ErrorCode::ModelNotFound,
                "The remote model was not found",
                false,
            ),
            K::ModelNotReady => (
                ErrorCode::ModelNotReady,
                "The remote model is not ready",
                true,
            ),
            K::UnsupportedAudio => (
                ErrorCode::UnsupportedAudio,
                "Remote audio is unsupported",
                false,
            ),
            K::WorkerFailed => (ErrorCode::WorkerFailed, "Remote recognition failed", true),
            K::Network => (ErrorCode::Network, "Fono Server connection failed", true),
            K::Timeout => (ErrorCode::Network, "Fono Server request timed out", true),
            K::InvalidResponse => (ErrorCode::Network, "Invalid Fono Server response", false),
            K::Internal => (
                ErrorCode::Internal,
                "Remote recognition could not be completed",
                false,
            ),
        };
        CoreError::new(code, message, retryable)
    }
}

fn failure<T>(kind: RemoteErrorKind) -> AppResult<T> {
    Err(RemoteError::new(kind).into())
}

/// Immutable discovery snapshot for one selected remote model. Device names
/// are remote (`remote:cuda:0`), never a claim about local GPU residency.
#[derive(Clone)]
pub struct RemoteRecognizer {
    transport: Arc<transport::Transport>,
    model: ModelId,
    wire_model: String,
    device: String,
    languages: Vec<String>,
    decoding: Vec<String>,
    capabilities: RecognizerCapabilities,
}

impl fmt::Debug for RemoteRecognizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteRecognizer").finish_non_exhaustive()
    }
}

impl RemoteRecognizer {
    pub fn connect(
        endpoint: RemoteEndpoint,
        model: ModelId,
        activity: Arc<dyn ActivityGuard>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Self> {
        recognizer::check_cancellation(cancellation)?;
        // Public IDs are identifiers, never a local path or arbitrary query.
        if model.variant.is_empty()
            || model.variant.len() > 128
            || !model
                .variant
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        let wire_model = match model.engine {
            ModelEngine::Whisper => format!("whisper:{}", model.variant),
            _ => model.public_id(),
        };
        let transport = Arc::new(transport::Transport::new(endpoint, activity)?);
        let http = transport.clone();
        let (server, models) =
            transport.call(cancellation, transport.discovery_deadline(), || {
                Ok(async move {
                    let server = http
                        .json::<wire::Capabilities>(http.get("v1/capabilities"), 64 * 1024)
                        .await?;
                    let models = http
                        .json::<wire::Models>(http.get("v1/models"), 512 * 1024)
                        .await?;
                    Ok((server, models))
                })
            })?;
        if server.product != "fono-server"
            || server.api_version < 2
            || !server.audio_formats.iter().any(|f| f == PCM_TYPE)
            || !server.features.cancel_on_disconnect
            || server.max_audio_seconds == 0
            || server.max_upload_bytes < 2
            || models.object != "list"
        {
            return failure(RemoteErrorKind::Incompatible);
        }
        let mut matches = models.data.into_iter().filter(|item| item.id == wire_model);
        let Some(selected) = matches.next() else {
            return failure(RemoteErrorKind::ModelNotFound);
        };
        if matches.next().is_some()
            || selected.object != "model"
            || selected.fono.engine != model.engine
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        let details = selected.fono;
        if !matches!(
            details.disk_state.as_str(),
            "verified" | "present_unverified"
        ) || details.state == "failed"
        {
            return failure(RemoteErrorKind::ModelNotReady);
        }
        let device = wire::device(&details.device)?;
        let max_segment_samples = (server.max_audio_seconds.saturating_mul(16_000))
            .min(server.max_upload_bytes / 2)
            .min((MAX_AUDIO_BYTES / 2) as u64) as usize;
        let timestamps = server.features.timestamps.iter().any(|v| v == "segments")
            && details.features.timestamps.iter().any(|v| v == "segments");
        let automatic_language = details.languages.iter().any(|v| v == "auto");
        let decoding: Vec<_> = details
            .features
            .decoding
            .into_iter()
            .filter(|v| server.features.decoding.contains(v))
            .collect();
        if !decoding.iter().any(|v| v == "fast") || max_segment_samples == 0 {
            return failure(RemoteErrorKind::Incompatible);
        }
        let decoding_options = decoding.iter().any(|v| v == "accurate");
        Ok(Self {
            transport,
            model,
            wire_model,
            device,
            languages: details.languages,
            decoding,
            capabilities: RecognizerCapabilities {
                max_segment_samples,
                window_transcription: timestamps,
                // The native route accepts glossary, not an initial prompt.
                initial_prompt: false,
                hotwords: server.features.glossary && details.features.glossary,
                language_detection: automatic_language,
                automatic_language,
                token_timestamps: false,
                decoding_options,
                cancellation: crate::CancellationSupport::Cooperative,
            },
        })
    }

    pub fn model(&self) -> &ModelId {
        &self.model
    }
    pub fn device_name(&self) -> String {
        self.device.clone()
    }
    pub fn capabilities(&self) -> RecognizerCapabilities {
        self.capabilities
    }
    pub fn is_idle(&self) -> bool {
        self.transport.is_idle()
    }
    fn matches_configuration(&self, endpoint: &RemoteEndpoint, model: &ModelId) -> bool {
        &self.model == model && self.transport.same_configuration(endpoint)
    }

    pub fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        let (result, request_ms) = self.recognize(samples, options, false, cancellation)?;
        let device = result
            .device
            .as_deref()
            .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?;
        let remote = RemoteTranscriptMetadata {
            model: self.model.clone(),
            backend: match result.backend.as_deref() {
                Some("cpu") => RemoteBackend::Cpu,
                Some("cuda") => RemoteBackend::Cuda,
                Some("vulkan") => RemoteBackend::Vulkan,
                _ => return failure(RemoteErrorKind::InvalidResponse),
            },
            device: device.into(),
            request_ms,
            server_total_ms: result.timings.total_ms,
            queue_ms: result.timings.queue_ms,
            load_ms: result.timings.load_ms,
            decode_ms: result.timings.decode_ms,
            network_ms: None,
        };
        if !remote.valid() {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        Ok(Transcript {
            text: result.text,
            detected_language: result.language,
            transcribe_secs: result
                .timings
                .decode_ms
                .map(|duration| duration as f32 / 1000.0),
            audio_secs: Some(samples.len() as f32 / 16_000.0),
            device: Some(wire::device(device)?),
            remote: Some(remote),
        })
    }

    pub fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        if options.context.is_some() || !self.capabilities.window_transcription {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        let (result, _) = self.recognize(samples, options.recognition, true, cancellation)?;
        let segments = result
            .segments
            .into_iter()
            .map(|s| {
                Ok(TimedSegment {
                    text: s.text,
                    start_sample: options
                        .audio_start_sample
                        .checked_add(s.start_ms * 16)
                        .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?,
                    end_sample: options
                        .audio_start_sample
                        .checked_add((s.end_ms * 16).min(samples.len() as u64))
                        .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?,
                })
            })
            .collect::<AppResult<_>>()?;
        Ok(WindowTranscript {
            text: result.text,
            detected_language: result.language,
            segments,
            words: vec![],
            transcribe_secs: result.timings.total_ms as f32 / 1000.0,
            audio_secs: samples.len() as f32 / 16_000.0,
            backend: match result.backend.as_deref() {
                Some("cpu") => BackendKind::Cpu,
                Some("cuda") => BackendKind::Cuda,
                Some("vulkan") => BackendKind::Vulkan,
                _ => return failure(RemoteErrorKind::InvalidResponse),
            },
        })
    }

    fn recognize(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        timestamps: bool,
        cancellation: &OperationCancellation,
    ) -> AppResult<(wire::Recognition, u64)> {
        recognizer::check_cancellation(cancellation)?;
        let strategy = match options.decoding.strategy {
            DecodingStrategy::Fast => "fast",
            DecodingStrategy::Accurate => "accurate",
        };
        if samples.is_empty()
            || samples.len() > self.capabilities.max_segment_samples
            || !self.languages.iter().any(|l| l == options.language)
            || options.decoding.threads.is_some()
            || !self.decoding.iter().any(|s| s == strategy)
            || (!options.glossary.is_empty() && !self.capabilities.hotwords)
        {
            return failure(RemoteErrorKind::InvalidRequest);
        }
        let http = self.transport.clone();
        let expected_model = self.wire_model.clone();
        let sample_count = samples.len();
        let started = Instant::now();
        let result =
            self.transport
                .call(cancellation, self.transport.deadline(sample_count), || {
                    // Admission is acquired before copying a bounded input. L16 is network byte order.
                    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
                    let mut request = http
                        .post("v1/recognize")
                        .header(reqwest::header::CONTENT_TYPE, PCM_TYPE)
                        .query(&[
                            ("model", self.wire_model.as_str()),
                            ("language", options.language),
                            ("timestamps", if timestamps { "segments" } else { "none" }),
                            ("decoding", strategy),
                            ("priority", "interactive"),
                        ])
                        .body(bytes);
                    if !options.glossary.is_empty() {
                        if options.glossary.len() > 32
                            || options.glossary.iter().any(|t| {
                                t.is_empty() || t.len() > 896 || t.chars().any(char::is_control)
                            })
                        {
                            return failure(RemoteErrorKind::InvalidRequest);
                        }
                        let json = serde_json::to_vec(options.glossary)
                            .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidRequest))?;
                        if json.len() > 4096 {
                            return failure(RemoteErrorKind::InvalidRequest);
                        }
                        let mut header =
                            reqwest::header::HeaderValue::from_str(&URL_SAFE_NO_PAD.encode(json))
                                .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidRequest))?;
                        header.set_sensitive(true);
                        request = request.header("x-fono-glossary", header);
                    }
                    Ok(async move {
                        let result = http
                            .json::<wire::Recognition>(request, 4 * 1024 * 1024)
                            .await?;
                        result.validate(&expected_model, sample_count)?;
                        Ok(result)
                    })
                })?;
        Ok((
            result,
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        ))
    }
}

impl Recognizer for RemoteRecognizer {
    fn load(request: &RecognizerLoad<'_>) -> AppResult<Self> {
        let crate::EngineCandidate::Remote { endpoint } = request.candidate else {
            return failure(RemoteErrorKind::InvalidRequest);
        };
        Self::connect(
            endpoint.as_ref().clone(),
            request.model.id.clone(),
            request.cleanup.activity.clone(),
            request.cancellation,
        )
    }
    fn model(&self) -> &ModelId {
        self.model()
    }
    fn device(&self) -> Cow<'static, str> {
        Cow::Owned(self.device_name())
    }
    fn capabilities(&self) -> RecognizerCapabilities {
        self.capabilities()
    }
    fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        self.transcribe(samples, options, cancellation)
    }
    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        self.transcribe_window(samples, options, cancellation)
    }
}

impl crate::SttEngine {
    /// Backward-compatible initial connection entry point.
    pub fn connect_remote(
        &self,
        endpoint: RemoteEndpoint,
        model: ModelId,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        self.ensure_remote_loaded(endpoint, &model, cancellation)
    }

    pub fn is_remote(&self) -> bool {
        matches!(&*self.state.lock(), crate::EngineState::Remote { .. })
    }

    /// Returning to an unconfigured local source must not retain an old
    /// remote route. It deliberately never unloads a local backend.
    pub fn clear_remote_if_idle(&self) -> AppResult<()> {
        let _gate = self.load_gate.lock();
        if !self.is_remote() {
            return Ok(());
        }
        let _source = self
            .source_gate
            .try_write()
            .ok_or_else(crate::source_change_busy)?;
        let _activity = self.environment.activity.lease()?;
        let previous = {
            let _publication = self.recovery_publication();
            let mut state = self.state.lock();
            if !crate::residency::idle_for_pool(&state) {
                return Err(crate::source_change_busy());
            }
            let previous = std::mem::replace(&mut *state, crate::EngineState::Empty);
            self.generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.clear_recovery_selection();
            drop(state);
            self.set_readiness(crate::SttReadiness::Unloaded);
            previous
        };
        drop(previous);
        Ok(())
    }

    /// Reuse an exact remote selection, or replace a quiescent source. Failed
    /// discovery leaves the previous backend intact. The host also fences
    /// recording, queues and settings persistence around this transaction.
    pub fn ensure_remote_loaded(
        &self,
        endpoint: RemoteEndpoint,
        model: &ModelId,
        cancellation: &OperationCancellation,
    ) -> AppResult<()> {
        let _gate = self.load_gate.lock();
        recognizer::check_cancellation(cancellation)?;
        self.reap_finished_worker();
        if self.cleanup.is_pending() {
            return Err(crate::worker_cleanup_busy());
        }
        let matching_device = {
            let state = self.state.lock();
            match &*state {
                crate::EngineState::Remote { client }
                    if client.matches_configuration(&endpoint, model) =>
                {
                    Some(client.device_name())
                }
                _ => None,
            }
        };
        if let Some(device) = matching_device {
            self.load_readiness(crate::SttReadiness::Ready { device }, None, 0)?;
            recognizer::check_cancellation(cancellation)?;
            return Ok(());
        }
        let _source = self
            .source_gate
            .try_write()
            .ok_or_else(crate::source_change_busy)?;
        if !crate::residency::idle_for_pool(&self.state.lock()) {
            return Err(crate::source_change_busy());
        }
        let _activity = self.environment.activity.lease()?;
        let selection = crate::types::SttModelSelection {
            id: model.clone(),
            local_path: String::new(),
        };
        let candidate = crate::EngineCandidate::Remote {
            endpoint: Box::new(endpoint),
        };
        let loaded = recognizer::load(RecognizerLoad {
            model: &selection,
            candidate: &candidate,
            cleanup: self.cleanup.clone(),
            cancellation,
            #[cfg(feature = "offline-stt")]
            speech_vad: None,
        })?;
        recognizer::check_cancellation(cancellation)?;
        let device = loaded.device();
        let previous = {
            let _publication = self.recovery_publication();
            recognizer::check_cancellation(cancellation)?;
            let mut state = self.state.lock();
            if !crate::residency::idle_for_pool(&state) {
                return Err(crate::source_change_busy());
            }
            let previous = std::mem::replace(&mut *state, crate::EngineState::Empty);
            self.generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.clear_recovery_selection();
            drop(state);
            self.set_readiness(crate::SttReadiness::Loading);
            previous
        };
        // Worker retirement/native destruction never owns the routing lock.
        // A pending worker remains observable and prevents new loads until
        // its supervisor confirms process exit and all stdio cleanup.
        if let crate::EngineState::Worker { session, .. } = &previous {
            if session.shutdown() == crate::worker::WorkerCleanupStatus::Pending {
                *self.state.lock() = previous;
                let error = crate::worker_cleanup_busy();
                self.set_readiness(crate::SttReadiness::Failed {
                    message: self.error_message(&error),
                });
                return Err(error);
            }
        }
        drop(previous);
        if self.cleanup.is_pending() {
            self.set_readiness(crate::SttReadiness::Unloaded);
            return Err(crate::worker_cleanup_busy());
        }
        if let Err(error) = recognizer::check_cancellation(cancellation) {
            self.set_readiness(crate::SttReadiness::Unloaded);
            return Err(error);
        }
        {
            let _publication = self.recovery_publication();
            if let Err(error) = recognizer::check_cancellation(cancellation) {
                self.set_readiness(crate::SttReadiness::Unloaded);
                return Err(error);
            }
            *self.state.lock() = loaded;
            self.set_readiness(crate::SttReadiness::Ready { device });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
