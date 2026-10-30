//! Synchronous HTTP recognition for the standalone server.
//!
//! The handler owns cancellation while a blocking worker owns the activity
//! lease. Dropping an HTTP future cancels work without pretending that native
//! decoding has already stopped.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::to_bytes;
use axum::extract::{rejection::QueryRejection, Extension, Query, Request};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use fono_core::OperationCancellation;
use fono_service::audio_ingest::{decode_file_cancellable, AudioIngestError, AudioIngestPolicy};
use fono_service::rest_api::{RestApiState, TokenPrincipal};
use fono_stt::scheduler::QueueClass;
use fono_stt::ActivityGuard;
use fono_types::{
    DecodingStrategy, SubtitleSegment, TranscriptionRequest, TranscriptionServiceError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jobs::{ServerActivity, ServerJobs};
use crate::recognition_options::{RecognitionOptions, RecognitionOutput, TimestampMode};

const REQUEST_ID_HEADER: &str = "x-request-id";
const GLOSSARY_HEADER: &str = "x-fono-glossary";

#[derive(Clone)]
pub struct RequestId(pub String);

/// A request id is assigned before authentication so error responses can be
/// correlated without recording a bearer token or audio content.
pub async fn request_id_middleware(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let header_value = HeaderValue::from_str(&id).expect("request id is restricted to safe ASCII");
    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER, header_value.clone());
    request.extensions_mut().insert(RequestId(id.clone()));
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER, header_value);
    response
}

pub type AdmittedRecognition = Box<
    dyn FnOnce(
            TranscriptionRequest,
            QueueClass,
            RecognitionOptions,
            OperationCancellation,
        ) -> Result<RecognitionOutput, TranscriptionServiceError>
        + Send,
>;

pub trait InteractiveRecognizer: Send + Sync {
    fn admit(
        self: Arc<Self>,
        client_key: &str,
    ) -> Result<AdmittedRecognition, TranscriptionServiceError>;
}

impl InteractiveRecognizer for ServerJobs {
    fn admit(
        self: Arc<Self>,
        client_key: &str,
    ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
        let admission = self.admit_client(client_key)?;
        Ok(Box::new(move |request, class, options, cancellation| {
            self.recognize_admitted_with_options(request, class, options, cancellation, admission)
        }))
    }
}

pub struct RecognizeState {
    recognizer: Arc<dyn InteractiveRecognizer>,
    activity: Arc<ServerActivity>,
    upload_dir: PathBuf,
    pub(crate) policy: AudioIngestPolicy,
    pub(crate) body_read_timeout: Duration,
}

impl RecognizeState {
    pub fn new(
        recognizer: Arc<dyn InteractiveRecognizer>,
        activity: Arc<ServerActivity>,
        upload_dir: PathBuf,
        policy: AudioIngestPolicy,
        body_read_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            recognizer,
            activity,
            upload_dir,
            policy,
            body_read_timeout,
        })
    }
}

pub fn routes(state: Arc<RecognizeState>) -> Router<RestApiState> {
    Router::new()
        .route("/v1/recognize", post(recognize))
        .merge(crate::openai::routes())
        .layer(Extension(state))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecognizeQuery {
    model: Option<String>,
    language: Option<String>,
    timestamps: Option<String>,
    decoding: Option<String>,
    priority: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum AudioFormat {
    Pcm,
    Wav,
    Opus,
    Mp3,
    Flac,
    Ogg,
}

#[derive(Serialize)]
struct RecognitionResponse {
    text: String,
    language: Option<String>,
    model: String,
    backend: Option<String>,
    device: Option<String>,
    segments: Vec<SubtitleSegment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    words: Vec<SubtitleSegment>,
    timings: RecognitionTimings,
    request_id: String,
}

#[derive(Serialize)]
struct RecognitionTimings {
    queue_ms: Option<u64>,
    load_ms: Option<u64>,
    decode_ms: Option<u64>,
    audio_ms: u64,
    total_ms: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: String,
}

#[derive(Debug)]
pub(crate) struct HttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
}

impl HttpError {
    pub(crate) fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "Credential request rate exceeded",
            retryable: true,
        }
    }
    pub(crate) fn invalid() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message: "Invalid recognition request",
            retryable: false,
        }
    }

    pub(crate) fn unsupported() -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            code: "unsupported_audio",
            message: "Unsupported audio format",
            retryable: false,
        }
    }

    pub(crate) fn too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large",
            message: "Audio exceeds the configured limit",
            retryable: false,
        }
    }

    pub(crate) fn body_timeout() -> Self {
        Self {
            status: StatusCode::REQUEST_TIMEOUT,
            code: "body_read_timeout",
            message: "Request body read exceeded the configured deadline",
            retryable: false,
        }
    }

    pub(crate) fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: "Recognition failed",
            retryable: false,
        }
    }

    pub(crate) fn into_response(self, request_id: &RequestId) -> Response {
        let mut response = (
            self.status,
            Json(ErrorResponse {
                error: ErrorDetail {
                    code: self.code,
                    message: self.message,
                    retryable: self.retryable,
                    request_id: request_id.0.clone(),
                },
            }),
        )
            .into_response();
        if self.retryable {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }

    pub(crate) fn into_openai_response(self, request_id: &RequestId) -> Response {
        let kind = if self.code == "rate_limited" {
            "rate_limit_error"
        } else if self.status.is_server_error() {
            "server_error"
        } else {
            "invalid_request_error"
        };
        let mut response = (
            self.status,
            Json(serde_json::json!({"error": {
                "message": self.message,
                "type": kind,
                "param": null,
                "code": self.code,
                "request_id": request_id.0
            }})),
        )
            .into_response();
        if self.retryable {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}

fn service_error(error: TranscriptionServiceError) -> HttpError {
    match error {
        TranscriptionServiceError::InvalidRequest(message)
            if message == "Selected model does not support the requested recognition options" =>
        {
            HttpError {
                status: StatusCode::BAD_REQUEST,
                code: "unsupported_options",
                message: "Selected model does not support the requested recognition options",
                retryable: false,
            }
        }
        TranscriptionServiceError::InvalidRequest(message)
            if message == "Selected model does not support the requested language" =>
        {
            HttpError {
                status: StatusCode::BAD_REQUEST,
                code: "unsupported_language",
                message: "Selected model does not support the requested language",
                retryable: false,
            }
        }
        TranscriptionServiceError::InvalidRequest(_) => HttpError::invalid(),
        TranscriptionServiceError::Busy(_) => HttpError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "busy",
            message: "Recognition queue is busy",
            retryable: true,
        },
        TranscriptionServiceError::ModelNotFound(_) => HttpError {
            status: StatusCode::NOT_FOUND,
            code: "model_not_found",
            message: "Recognition model is not installed",
            retryable: false,
        },
        TranscriptionServiceError::ModelNotReady(_) => HttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "model_not_ready",
            message: "Recognition model is not ready",
            retryable: true,
        },
        TranscriptionServiceError::Cancelled(_) => HttpError {
            status: StatusCode::CONFLICT,
            code: "cancelled",
            message: "Recognition was cancelled",
            retryable: false,
        },
        TranscriptionServiceError::Failed(_) => HttpError::internal(),
        TranscriptionServiceError::WorkerFailed(_) => HttpError {
            status: StatusCode::BAD_GATEWAY,
            code: "worker_failed",
            message: "Recognition worker failed",
            retryable: true,
        },
    }
}

fn ingest_error(error: AudioIngestError) -> HttpError {
    match error {
        AudioIngestError::UnsupportedAudio(_) | AudioIngestError::CorruptedAudio(_) => {
            HttpError::unsupported()
        }
        AudioIngestError::FileTooLarge(_) | AudioIngestError::DurationLimit(_) => {
            HttpError::too_large()
        }
        AudioIngestError::Cancelled(_) => service_error(TranscriptionServiceError::Cancelled(
            "audio preparation cancelled".into(),
        )),
        AudioIngestError::Io(_) => HttpError::internal(),
    }
}

struct CancelOnDrop(OperationCancellation);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn recognize(
    Extension(state): Extension<Arc<RecognizeState>>,
    Extension(principal): Extension<TokenPrincipal>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    query: Result<Query<RecognizeQuery>, QueryRejection>,
    request: Request,
) -> Response {
    let started = Instant::now();
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return HttpError::invalid().into_response(&request_id),
    };
    let format = match audio_format(&headers) {
        Ok(format) => format,
        Err(error) => return error.into_response(&request_id),
    };
    let (language, model, class, options) = match validate_query(&query, &headers) {
        Ok(parameters) => parameters,
        Err(error) => return error.into_response(&request_id),
    };
    let admission = match admit(&state, principal.client_key).await {
        Ok(admission) => admission,
        Err(error) => return error.into_response(&request_id),
    };
    let max_file_bytes = usize::try_from(state.policy.max_file_bytes).unwrap_or(usize::MAX);
    let body = match tokio::time::timeout(
        state.body_read_timeout,
        to_bytes(request.into_body(), max_file_bytes),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return HttpError::too_large().into_response(&request_id),
        Err(_) => return HttpError::body_timeout().into_response(&request_id),
    };
    if body.is_empty() {
        return HttpError::invalid().into_response(&request_id);
    }
    if body.len() as u64 > state.policy.max_file_bytes {
        return HttpError::too_large().into_response(&request_id);
    }
    match execute(
        state,
        admission,
        RecognitionInput {
            body: body.to_vec(),
            format,
            language,
            model,
            class,
            options,
        },
    )
    .await
    {
        Ok(completed) => {
            let CompletedRecognition {
                output,
                audio_ms,
                requested_language,
            } = completed;
            let result = output.result;
            let language = result
                .detected_language
                .or_else(|| (requested_language != "auto").then_some(requested_language));
            let backend = normalize_backend(result.backend);
            let device = output.device.or_else(|| backend.clone());
            Json(RecognitionResponse {
                text: result.text,
                language,
                model: result.model,
                backend,
                device,
                segments: result.segments,
                words: output.words,
                timings: RecognitionTimings {
                    queue_ms: None,
                    load_ms: None,
                    decode_ms: None,
                    audio_ms,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                request_id: request_id.0,
            })
            .into_response()
        }
        Err(error) => error.into_response(&request_id),
    }
}

pub(crate) struct RecognitionInput {
    pub body: Vec<u8>,
    pub format: AudioFormat,
    pub language: String,
    pub model: Option<String>,
    pub class: QueueClass,
    pub options: RecognitionOptions,
}

pub(crate) struct CompletedRecognition {
    pub output: RecognitionOutput,
    pub audio_ms: u64,
    pub requested_language: String,
}

/// Admission happens before receiving bytes, including multipart uploads.
pub(crate) async fn admit(
    state: &Arc<RecognizeState>,
    client_key: String,
) -> Result<AdmittedRecognition, HttpError> {
    let recognizer = Arc::clone(&state.recognizer);
    match tokio::task::spawn_blocking(move || recognizer.admit(&client_key)).await {
        Ok(result) => result.map_err(service_error),
        Err(_) => Err(HttpError::internal()),
    }
}

/// Shared execution for native and OpenAI-style HTTP inputs. The cancellation
/// guard belongs to the waiting HTTP future; the lease belongs to real work.
pub(crate) async fn execute(
    state: Arc<RecognizeState>,
    admission: AdmittedRecognition,
    input: RecognitionInput,
) -> Result<CompletedRecognition, HttpError> {
    let cancellation = OperationCancellation::default();
    let _cancel_on_drop = CancelOnDrop(cancellation.clone());
    let worker = tokio::task::spawn_blocking(move || {
        let _activity = state.activity.lease().map_err(|_| {
            service_error(TranscriptionServiceError::Busy(
                "server is shutting down".into(),
            ))
        })?;
        let started = std::time::Instant::now();
        let samples = decode_audio(
            &input.body,
            input.format,
            &state.upload_dir,
            state.policy,
            &cancellation,
        );
        state
            .activity
            .observability
            .audio_decoded(started.elapsed());
        let samples = samples?;
        let audio_ms = samples.len() as u64 * 1_000 / 16_000;
        let output = admission(
            TranscriptionRequest {
                pcm_samples: samples,
                language: input.language.clone(),
                model: input.model,
            },
            input.class,
            input.options,
            cancellation,
        )
        .map_err(service_error)?;
        Ok(CompletedRecognition {
            output,
            audio_ms,
            requested_language: input.language,
        })
    });
    worker.await.map_err(|_| HttpError::internal())?
}

fn normalize_backend(backend: Option<String>) -> Option<String> {
    backend.map(|value| match value.as_str() {
        "CPU" => "cpu".to_owned(),
        "CUDA" => "cuda".to_owned(),
        "Vulkan" => "vulkan".to_owned(),
        _ => value,
    })
}

fn validate_query(
    query: &RecognizeQuery,
    headers: &HeaderMap,
) -> Result<(String, Option<String>, QueueClass, RecognitionOptions), HttpError> {
    let timestamps = match query.timestamps.as_deref() {
        None | Some("none") => TimestampMode::None,
        Some("segments") => TimestampMode::Segments,
        Some("words") => TimestampMode::Words,
        _ => return Err(HttpError::invalid()),
    };
    let strategy = match query.decoding.as_deref() {
        None | Some("fast") => DecodingStrategy::Fast,
        Some("accurate") => DecodingStrategy::Accurate,
        _ => return Err(HttpError::invalid()),
    };
    let language = query.language.as_deref().unwrap_or("auto");
    if !matches!(language, "auto" | "ru" | "en") {
        return Err(HttpError::invalid());
    }
    if query
        .model
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 128)
    {
        return Err(HttpError::invalid());
    }
    let class = match query.priority.as_deref() {
        None | Some("interactive") => QueueClass::Interactive,
        Some("batch") => QueueClass::Batch,
        _ => return Err(HttpError::invalid()),
    };
    let options = RecognitionOptions {
        decoding: fono_types::DecodingOptions {
            strategy,
            ..Default::default()
        },
        timestamps,
        glossary: parse_glossary(headers)?,
        ..RecognitionOptions::default()
    };
    Ok((language.to_owned(), query.model.clone(), class, options))
}

fn parse_glossary(headers: &HeaderMap) -> Result<Vec<String>, HttpError> {
    let mut values = headers.get_all(GLOSSARY_HEADER).iter();
    let value = match (values.next(), values.next()) {
        (None, None) => return Ok(Vec::new()),
        (Some(value), None) => value.to_str().map_err(|_| HttpError::invalid())?,
        _ => return Err(HttpError::invalid()),
    };
    // Bound encoded bytes before decoding so a header cannot allocate freely.
    if value.len() > 5464 {
        return Err(HttpError::invalid());
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(value))
        .map_err(|_| HttpError::invalid())?;
    if decoded.len() > 4096 {
        return Err(HttpError::invalid());
    }
    let terms: Vec<String> = serde_json::from_slice(&decoded).map_err(|_| HttpError::invalid())?;
    fono_stt_protocol::prompt::validate(&terms).map_err(|_| HttpError::invalid())?;
    Ok(terms)
}

fn audio_format(headers: &HeaderMap) -> Result<AudioFormat, HttpError> {
    let value = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(HttpError::unsupported)?;
    let mut parts = value.split(';');
    let media = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let mut parameters = Vec::new();
    for parameter in parts {
        let (key, value) = parameter
            .split_once('=')
            .ok_or_else(HttpError::unsupported)?;
        parameters.push((
            key.trim().to_ascii_lowercase(),
            value.trim().trim_matches('"').to_ascii_lowercase(),
        ));
    }
    match media.as_str() {
        "audio/l16"
            if parameters.len() == 2
                && parameters
                    .iter()
                    .any(|(key, value)| key == "rate" && value == "16000")
                && parameters
                    .iter()
                    .any(|(key, value)| key == "channels" && value == "1") =>
        {
            Ok(AudioFormat::Pcm)
        }
        "audio/wav" if parameters.is_empty() => Ok(AudioFormat::Wav),
        "audio/ogg"
            if parameters.len() == 1
                && parameters[0].0 == "codecs"
                && parameters[0].1 == "opus" =>
        {
            Ok(AudioFormat::Opus)
        }
        _ => Err(HttpError::unsupported()),
    }
}

fn decode_audio(
    body: &[u8],
    format: AudioFormat,
    directory: &Path,
    policy: AudioIngestPolicy,
    cancellation: &OperationCancellation,
) -> Result<Vec<i16>, HttpError> {
    if cancellation.is_cancelled() {
        return Err(service_error(TranscriptionServiceError::Cancelled(
            "request cancelled".into(),
        )));
    }
    if let AudioFormat::Pcm = format {
        if !body.len().is_multiple_of(2) || body.is_empty() {
            return Err(HttpError::invalid());
        }
        let max_samples = policy.max_duration_seconds as usize * 16_000;
        if body.len() / 2 > max_samples {
            return Err(HttpError::too_large());
        }
        return Ok(body
            .chunks_exact(2)
            .map(|pair| i16::from_be_bytes([pair[0], pair[1]]))
            .collect());
    }
    let extension = match format {
        AudioFormat::Wav
            if body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WAVE" =>
        {
            "wav"
        }
        AudioFormat::Opus if is_ogg_opus(body) => "ogg",
        AudioFormat::Mp3 => "mp3",
        AudioFormat::Flac => "flac",
        AudioFormat::Ogg => "ogg",
        _ => return Err(HttpError::unsupported()),
    };
    let path = directory.join(format!(
        "recognize-{}.{}",
        Uuid::new_v4().simple(),
        extension
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|_| HttpError::internal())?;
    let _cleanup = TempAudio(path.clone());
    for chunk in body.chunks(64 * 1024) {
        if cancellation.is_cancelled() {
            return Err(service_error(TranscriptionServiceError::Cancelled(
                "request cancelled".into(),
            )));
        }
        file.write_all(chunk).map_err(|_| HttpError::internal())?;
    }
    drop(file);
    let normalized = decode_file_cancellable(&path, policy, cancellation).map_err(ingest_error)?;
    Ok(normalized.pcm_samples)
}

fn is_ogg_opus(body: &[u8]) -> bool {
    if body.len() < 27 || !body.starts_with(b"OggS") {
        return false;
    }
    let segment_count = body[26] as usize;
    let first_packet = 27 + segment_count;
    body.get(first_packet..first_packet + 8) == Some(b"OpusHead".as_slice())
}

struct TempAudio(PathBuf);

impl Drop for TempAudio {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_requires_exact_audio_contract() {
        let mut headers = HeaderMap::new();
        for valid in [
            "audio/L16; rate=16000; channels=1",
            "audio/L16; channels=1; rate=16000",
            "audio/wav",
            "audio/ogg; codecs=opus",
        ] {
            headers.insert(header::CONTENT_TYPE, valid.parse().unwrap());
            assert!(audio_format(&headers).is_ok(), "{valid}");
        }
        for invalid in [
            "audio/L16",
            "audio/L16; rate=48000; channels=1",
            "audio/L16; rate=16000; channels=2",
            "audio/ogg",
            "audio/ogg; codecs=vorbis",
            "audio/mp3",
        ] {
            headers.insert(header::CONTENT_TYPE, invalid.parse().unwrap());
            assert!(audio_format(&headers).is_err(), "{invalid}");
        }
    }

    #[test]
    fn raw_l16_pcm_uses_network_byte_order_and_duration_bound() {
        let policy = AudioIngestPolicy {
            max_file_bytes: 1024,
            max_duration_seconds: 1,
        };
        let samples = decode_audio(
            &[0x12, 0x34, 0xff, 0xfe, 0x80, 0x00, 0x7f, 0xff],
            AudioFormat::Pcm,
            Path::new("."),
            policy,
            &OperationCancellation::default(),
        )
        .unwrap();
        assert_eq!(samples, vec![0x1234, -2, i16::MIN, i16::MAX]);
        assert!(decode_audio(
            &[0x01],
            AudioFormat::Pcm,
            Path::new("."),
            policy,
            &OperationCancellation::default(),
        )
        .is_err());
        assert!(decode_audio(
            &vec![0; 16_001 * 2],
            AudioFormat::Pcm,
            Path::new("."),
            policy,
            &OperationCancellation::default(),
        )
        .is_err());
    }

    #[test]
    fn api_backend_uses_canonical_device_names() {
        assert_eq!(
            normalize_backend(Some("CPU".into())).as_deref(),
            Some("cpu")
        );
        assert_eq!(
            normalize_backend(Some("CUDA".into())).as_deref(),
            Some("cuda")
        );
        assert_eq!(
            normalize_backend(Some("Vulkan".into())).as_deref(),
            Some("vulkan")
        );
        assert_eq!(
            normalize_backend(Some("custom".into())).as_deref(),
            Some("custom")
        );
        assert_eq!(normalize_backend(None), None);
    }

    #[test]
    fn priority_is_interactive_by_default_and_batch_when_requested() {
        let headers = HeaderMap::new();
        let default = RecognizeQuery::default();
        assert_eq!(
            validate_query(&default, &headers).unwrap().2,
            QueueClass::Interactive
        );
        let batch = RecognizeQuery {
            priority: Some("batch".into()),
            ..RecognizeQuery::default()
        };
        assert_eq!(
            validate_query(&batch, &headers).unwrap().2,
            QueueClass::Batch
        );
    }

    #[test]
    fn query_carries_advanced_decoding_real_timestamps_and_bounded_glossary() {
        let mut headers = HeaderMap::new();
        let encoded =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"["Fono","Kubernetes"]"#);
        headers.insert(GLOSSARY_HEADER, encoded.parse().unwrap());
        let query = RecognizeQuery {
            timestamps: Some("words".into()),
            decoding: Some("accurate".into()),
            ..RecognizeQuery::default()
        };
        let (_, _, _, options) = validate_query(&query, &headers).unwrap();
        assert!(matches!(options.timestamps, TimestampMode::Words));
        assert_eq!(options.decoding.strategy, DecodingStrategy::Accurate);
        assert_eq!(options.glossary, ["Fono", "Kubernetes"]);
        for invalid in [
            "not-base64!".into(),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"["bad\nterm"]"#),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("x".repeat(4097)),
        ] {
            headers.insert(GLOSSARY_HEADER, invalid.parse().unwrap());
            assert!(validate_query(&query, &headers).is_err());
        }
        headers.insert(GLOSSARY_HEADER, encoded.parse().unwrap());
        headers.append(GLOSSARY_HEADER, encoded.parse().unwrap());
        assert!(validate_query(&query, &headers).is_err());
    }
}
