//! Transport-neutral HTTP surface for transcription. The host chooses the
//! listener and owns authentication, activity admission and shutdown.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::audio_ingest::{self, AudioIngestError, AudioIngestPolicy};
use crate::transcription_jobs::{JobReservation, TranscriptionJobs};
use axum::{
    extract::{DefaultBodyLimit, FromRequest, Multipart, Path as AxumPath, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use fono_stt::ActivityGuard;
use fono_types::transcription_contract::{TranscriptionRequest, TranscriptionServiceError};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const UPLOAD_BODY_LIMIT_BYTES: usize = 101 * 1024 * 1024;
const MAX_TEXT_FIELD_BYTES: usize = 64;
const OPENAPI_SPEC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json"));
const API_DOCUMENTATION: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/api-docs.html"));

#[derive(Clone)]
pub struct RestApiState {
    verifier: Arc<dyn TokenVerifier>,
    activity: Arc<dyn ActivityGuard>,
    pub protocol_version: u16,
    jobs: Option<Arc<dyn TranscriptionJobs>>,
    upload_dir: Arc<PathBuf>,
    upload_policy: AudioIngestPolicy,
    body_read_timeout: Option<Duration>,
}

/// The desktop supplies one token; a server host can implement token-file
/// verification with an individual scope for each client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenScope {
    Recognize,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAuthorization {
    Allowed,
    Unauthorized,
    Forbidden,
}

/// Opaque, nonsecret identity used for per-client admission. Hosts that keep
/// multiple bearer tokens override `principal` to return a distinct key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPrincipal {
    pub client_key: String,
    pub scope: TokenScope,
}

pub trait TokenVerifier: Send + Sync {
    fn verify(&self, token: &str) -> bool;

    /// The existing desktop token has full local access. Server token stores
    /// override this to return each client's persisted scope.
    fn scope(&self, token: &str) -> Option<TokenScope> {
        self.verify(token).then_some(TokenScope::Admin)
    }

    fn principal(&self, token: &str) -> Option<TokenPrincipal> {
        self.scope(token).map(|scope| TokenPrincipal {
            client_key: "desktop".into(),
            scope,
        })
    }
}

pub struct StaticTokenVerifier(Arc<str>);

impl StaticTokenVerifier {
    pub fn new(token: impl Into<Arc<str>>) -> Self {
        Self(token.into())
    }
}

impl TokenVerifier for StaticTokenVerifier {
    fn verify(&self, token: &str) -> bool {
        let expected = self.0.as_bytes();
        let supplied = token.as_bytes();
        if expected.len() != supplied.len() {
            return false;
        }
        let mut difference = 0u8;
        for (left, right) in expected.iter().zip(supplied) {
            difference |= left ^ right;
        }
        difference == 0
    }
}

impl RestApiState {
    pub fn new(
        token: impl Into<Arc<str>>,
        protocol_version: u16,
        activity: Arc<dyn ActivityGuard>,
    ) -> Self {
        Self {
            verifier: Arc::new(StaticTokenVerifier::new(token)),
            activity,
            protocol_version,
            jobs: None,
            upload_dir: Arc::new(std::env::temp_dir().join("fono-transcription-uploads")),
            upload_policy: AudioIngestPolicy::default(),
            body_read_timeout: None,
        }
    }

    pub fn with_token_verifier(mut self, verifier: Arc<dyn TokenVerifier>) -> Self {
        self.verifier = verifier;
        self
    }

    pub fn with_jobs(mut self, jobs: Arc<dyn TranscriptionJobs>) -> Self {
        self.jobs = Some(jobs);
        self
    }

    pub fn with_upload_dir(mut self, upload_dir: PathBuf) -> Self {
        self.upload_dir = Arc::new(upload_dir);
        self
    }

    pub fn with_upload_policy(mut self, policy: AudioIngestPolicy) -> Self {
        self.upload_policy = policy;
        self
    }

    /// The host may bound total HTTP body reception without timing native
    /// decoding or recognition. Desktop callers keep the existing default.
    pub fn with_body_read_timeout(mut self, timeout: Duration) -> Self {
        self.body_read_timeout = Some(timeout);
        self
    }
}

#[derive(Serialize)]
struct Health {
    protocol_version: u16,
    state: &'static str,
}

#[derive(Serialize)]
struct ApiError {
    code: &'static str,
}

#[derive(Serialize)]
struct ApiErrorV2 {
    error: ApiErrorV2Detail,
}

#[derive(Serialize)]
struct ApiErrorV2Detail {
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: String,
}

pub fn router(state: RestApiState) -> Router {
    meta_routes()
        .merge(jobs_routes())
        .merge(recognize_routes())
        .merge(openai_routes())
        .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT_BYTES))
        .with_state(state)
}

pub fn meta_routes() -> Router<RestApiState> {
    Router::new()
        .route("/docs", get(docs))
        .route("/openapi.json", get(openapi))
        .route("/v1/health", get(health))
}

pub fn jobs_routes() -> Router<RestApiState> {
    Router::new()
        .route("/v1/transcription-jobs", post(submit))
        .route("/v1/transcriptions", post(upload))
        .route("/v1/transcription-jobs/:id", get(job))
        .route("/v1/transcription-jobs/:id/cancel", post(cancel))
}

/// API-02 adds synchronous recognition here without changing the jobs router.
pub fn recognize_routes() -> Router<RestApiState> {
    Router::new()
}

/// API-06 adds OpenAI-compatible transcription here.
pub fn openai_routes() -> Router<RestApiState> {
    Router::new()
}

async fn docs() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        API_DOCUMENTATION,
    )
}

/// Serve on the caller's already-bound listener. No address policy or runtime
/// is hidden inside this crate.
pub async fn serve(router: Router, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router).await
}

pub async fn serve_with_shutdown(
    router: Router,
    listener: tokio::net::TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Authenticate only a single Bearer header and distinguish a missing or
/// revoked token from a valid token without the required scope. No token value
/// is included in the result or logged by this helper.
pub fn authorize_headers(
    headers: &HeaderMap,
    verifier: &dyn TokenVerifier,
    required: TokenScope,
) -> TokenAuthorization {
    match authorize_principal_headers(headers, verifier, required) {
        Ok(_) => TokenAuthorization::Allowed,
        Err(error) => error,
    }
}

pub fn authorize_principal_headers(
    headers: &HeaderMap,
    verifier: &dyn TokenVerifier,
    required: TokenScope,
) -> Result<TokenPrincipal, TokenAuthorization> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Err(TokenAuthorization::Unauthorized);
    };
    if values.next().is_some() {
        return Err(TokenAuthorization::Unauthorized);
    }
    let Some(token) = value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(TokenAuthorization::Unauthorized);
    };
    if token.is_empty() || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(TokenAuthorization::Unauthorized);
    }
    match verifier.principal(token) {
        None => Err(TokenAuthorization::Unauthorized),
        Some(principal) if required == TokenScope::Recognize => Ok(principal),
        Some(principal) if principal.scope == TokenScope::Admin => Ok(principal),
        Some(_) => Err(TokenAuthorization::Forbidden),
    }
}

fn authorized(headers: &HeaderMap, state: &RestApiState) -> bool {
    authorize_headers(headers, state.verifier.as_ref(), TokenScope::Recognize)
        == TokenAuthorization::Allowed
}

fn api_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(ApiError { code })).into_response()
}

fn submission_error(
    service_error: TranscriptionServiceError,
    protocol_version: u16,
    headers: &HeaderMap,
) -> axum::response::Response {
    let (status, code, message, retryable) = match service_error {
        TranscriptionServiceError::InvalidRequest(_) => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid transcription request",
            false,
        ),
        TranscriptionServiceError::Busy(_) => (
            StatusCode::TOO_MANY_REQUESTS,
            if protocol_version >= 2 {
                "busy"
            } else {
                "queue_full"
            },
            "Recognition queue is busy",
            true,
        ),
        TranscriptionServiceError::ModelNotReady(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "model_not_ready",
            "Recognition model is not ready",
            true,
        ),
        TranscriptionServiceError::ModelNotFound(_) => (
            StatusCode::NOT_FOUND,
            "model_not_found",
            "Recognition model is not installed",
            false,
        ),
        TranscriptionServiceError::WorkerFailed(_) => (
            StatusCode::BAD_GATEWAY,
            "worker_failed",
            "Recognition worker failed",
            true,
        ),
        TranscriptionServiceError::Cancelled(_) => (
            StatusCode::CONFLICT,
            "cancelled",
            "Transcription was cancelled",
            false,
        ),
        TranscriptionServiceError::Failed(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            if protocol_version >= 2 {
                "internal"
            } else {
                "transcription_failed"
            },
            "Transcription failed",
            false,
        ),
    };
    versioned_error(status, code, message, retryable, protocol_version, headers)
}

fn body_timeout_error(protocol_version: u16, headers: &HeaderMap) -> axum::response::Response {
    versioned_error(
        StatusCode::REQUEST_TIMEOUT,
        "body_read_timeout",
        "Request body read exceeded the configured deadline",
        false,
        protocol_version,
        headers,
    )
}

fn versioned_error(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    protocol_version: u16,
    headers: &HeaderMap,
) -> axum::response::Response {
    if protocol_version < 2 {
        return api_error(status, code);
    }
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut response = (
        status,
        Json(ApiErrorV2 {
            error: ApiErrorV2Detail {
                code,
                message,
                retryable,
                request_id: request_id.clone(),
            },
        }),
    )
        .into_response();
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id).expect("request id came from a valid header"),
    );
    if retryable {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

async fn submit(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    body: Request,
) -> impl IntoResponse {
    let principal =
        match authorize_principal_headers(&headers, state.verifier.as_ref(), TokenScope::Recognize)
        {
            Ok(principal) => principal,
            Err(_) => return api_error(StatusCode::UNAUTHORIZED, "unauthorized"),
        };
    let reservation = match state
        .jobs
        .as_ref()
        .map(|jobs| jobs.reserve_for_client(&principal.client_key))
        .transpose()
    {
        Ok(reservation) => reservation,
        Err(error) => return submission_error(error, state.protocol_version, &headers),
    };
    let _activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(_) => return api_error(StatusCode::SERVICE_UNAVAILABLE, "update_installing"),
    };
    let extraction = Json::<TranscriptionRequest>::from_request(body, &state);
    let parsed = match state.body_read_timeout {
        Some(timeout) => match tokio::time::timeout(timeout, extraction).await {
            Ok(parsed) => parsed,
            Err(_) => return body_timeout_error(state.protocol_version, &headers),
        },
        None => extraction.await,
    };
    let Json(request) = match parsed {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let Some(jobs) = state.jobs.as_ref() else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    let reservation = reservation.expect("job reservation exists when jobs are configured");
    match jobs.submit_job_reserved(reservation, request) {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => submission_error(error, state.protocol_version, &headers),
    }
}

async fn upload(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> axum::response::Response {
    let principal =
        match authorize_principal_headers(&headers, state.verifier.as_ref(), TokenScope::Recognize)
        {
            Ok(principal) => principal,
            Err(_) => return api_error(StatusCode::UNAUTHORIZED, "unauthorized"),
        };
    let reservation = match state
        .jobs
        .as_ref()
        .map(|jobs| jobs.reserve_for_client(&principal.client_key))
        .transpose()
    {
        Ok(reservation) => reservation,
        Err(error) => return submission_error(error, state.protocol_version, &headers),
    };
    let _activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(_) => return api_error(StatusCode::SERVICE_UNAVAILABLE, "update_installing"),
    };
    let (request, reservation) = match request_from_multipart(
        multipart,
        &state.upload_dir,
        &state.activity,
        state.upload_policy,
        state.body_read_timeout,
        reservation,
    )
    .await
    {
        Ok(request) => request,
        Err(error) => return upload_error(error, state.protocol_version, &headers),
    };
    let Some(jobs) = state.jobs.as_ref() else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    let reservation = reservation.expect("job reservation exists when jobs are configured");
    match jobs.submit_job_reserved(reservation, request) {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => submission_error(error, state.protocol_version, &headers),
    }
}

async fn job(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    state
        .jobs
        .as_ref()
        .and_then(|jobs| jobs.get_job(&id))
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|| api_error(StatusCode::NOT_FOUND, "job_not_found"))
}

async fn cancel(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    state
        .jobs
        .as_ref()
        .and_then(|jobs| jobs.cancel_job(&id))
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|| api_error(StatusCode::NOT_FOUND, "job_not_found"))
}

async fn request_from_multipart(
    multipart: Multipart,
    upload_dir: &Path,
    activity: &Arc<dyn ActivityGuard>,
    policy: AudioIngestPolicy,
    body_read_timeout: Option<Duration>,
    reservation: Option<JobReservation>,
) -> Result<(TranscriptionRequest, Option<JobReservation>), UploadError> {
    let read_body = read_multipart_body(multipart, upload_dir, policy);
    let (language, model, audio_path) = match body_read_timeout {
        Some(timeout) => tokio::time::timeout(timeout, read_body)
            .await
            .map_err(|_| UploadError::TimedOut)??,
        None => read_body.await?,
    };
    let (decoded, reservation) = tokio::task::spawn_blocking({
        let activity = activity
            .lease()
            .map_err(|_| UploadError::Internal("application update is installing".into()))?;
        move || {
            let _activity = activity;
            // The file stays owned by the decoder even if the HTTP future
            // is dropped while this blocking work is still running.
            (
                audio_ingest::decode_file(&audio_path.0, policy),
                reservation,
            )
        }
    })
    .await
    .map_err(|error| UploadError::Internal(format!("audio decoder join failed: {error}")))?;
    let audio = decoded.map_err(UploadError::Audio)?;
    Ok((
        audio.into_transcription_request(language, model),
        reservation,
    ))
}

async fn read_multipart_body(
    mut multipart: Multipart,
    upload_dir: &Path,
    policy: AudioIngestPolicy,
) -> Result<(String, Option<String>, UploadedFile), UploadError> {
    let mut language = "auto".to_string();
    let mut model = None;
    let mut audio_path = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| UploadError::Invalid(error.to_string()))?
    {
        match field.name() {
            Some("language") => language = bounded_text(field).await?,
            Some("model") => model = Some(bounded_text(field).await?),
            Some("audio") if audio_path.is_none() => {
                let extension = upload_extension(field.file_name())?;
                audio_path = Some(write_upload(&mut field, upload_dir, &extension, policy).await?);
            }
            Some("audio") => {
                return Err(UploadError::Invalid(
                    "only one audio file is allowed".into(),
                ))
            }
            _ => return Err(UploadError::Invalid("unsupported multipart field".into())),
        }
    }
    let audio_path =
        audio_path.ok_or_else(|| UploadError::Invalid("audio field is required".into()))?;
    Ok((language, model, audio_path))
}

struct UploadedFile(PathBuf);

impl Drop for UploadedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn bounded_text(
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<String, UploadError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| UploadError::Invalid(error.to_string()))?
    {
        if chunk.len() > MAX_TEXT_FIELD_BYTES.saturating_sub(bytes.len()) {
            return Err(UploadError::Invalid(
                "text fields must contain 1 through 64 bytes".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| UploadError::Invalid("text fields must be UTF-8".into()))?;
    if text.trim().is_empty() {
        return Err(UploadError::Invalid(
            "text fields must contain 1 through 64 bytes".into(),
        ));
    }
    Ok(text)
}

async fn write_upload(
    field: &mut axum::extract::multipart::Field<'_>,
    upload_dir: &Path,
    extension: &str,
    policy: AudioIngestPolicy,
) -> Result<UploadedFile, UploadError> {
    tokio::fs::create_dir_all(upload_dir)
        .await
        .map_err(|error| UploadError::Internal(error.to_string()))?;
    let path =
        UploadedFile(upload_dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), extension)));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path.0)
        .await
        .map_err(|error| UploadError::Internal(error.to_string()))?;
    let mut bytes_written = 0_u64;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| UploadError::Invalid(error.to_string()))?
    {
        bytes_written = bytes_written.saturating_add(chunk.len() as u64);
        if bytes_written > policy.max_file_bytes {
            return Err(UploadError::TooLarge);
        }
        if let Err(error) = file.write_all(&chunk).await {
            return Err(UploadError::Internal(error.to_string()));
        }
    }
    if let Err(error) = file.flush().await {
        return Err(UploadError::Internal(error.to_string()));
    }
    drop(file);
    Ok(path)
}

fn upload_extension(file_name: Option<&str>) -> Result<String, UploadError> {
    let extension = file_name
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            UploadError::Invalid("audio filename with an extension is required".into())
        })?;
    match extension.as_str() {
        "wav" | "mp3" | "flac" | "ogg" => Ok(extension),
        _ => Err(UploadError::Unsupported),
    }
}

enum UploadError {
    Invalid(String),
    TimedOut,
    TooLarge,
    Unsupported,
    Audio(AudioIngestError),
    Internal(String),
}

fn upload_error(
    error: UploadError,
    protocol_version: u16,
    headers: &HeaderMap,
) -> axum::response::Response {
    match error {
        UploadError::TimedOut => body_timeout_error(protocol_version, headers),
        UploadError::Invalid(message) => {
            let _ = message;
            tracing::warn!("invalid transcription upload");
            api_error(StatusCode::BAD_REQUEST, "invalid_upload")
        }
        UploadError::TooLarge | UploadError::Audio(AudioIngestError::FileTooLarge(_)) => {
            api_error(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large")
        }
        UploadError::Unsupported | UploadError::Audio(AudioIngestError::UnsupportedAudio(_)) => {
            api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_audio")
        }
        UploadError::Audio(AudioIngestError::DurationLimit(_)) => {
            api_error(StatusCode::UNPROCESSABLE_ENTITY, "duration_limit")
        }
        UploadError::Audio(AudioIngestError::CorruptedAudio(_)) => {
            api_error(StatusCode::UNPROCESSABLE_ENTITY, "corrupted_audio")
        }
        UploadError::Audio(AudioIngestError::Cancelled(_)) => {
            api_error(StatusCode::CONFLICT, "preparation_cancelled")
        }
        UploadError::Audio(AudioIngestError::Io(message)) | UploadError::Internal(message) => {
            let _ = message;
            tracing::error!("transcription upload failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed")
        }
    }
}

async fn health(State(state): State<RestApiState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    (
        StatusCode::OK,
        Json(Health {
            protocol_version: state.protocol_version,
            state: "ready",
        }),
    )
        .into_response()
}

async fn openapi(State(state): State<RestApiState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_SPEC,
    )
        .into_response()
}

#[cfg(test)]
mod tests;
