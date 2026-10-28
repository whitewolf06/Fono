//! Transport-neutral HTTP surface for transcription. The host chooses the
//! listener and owns authentication, activity admission and shutdown.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::audio_ingest::{self, AudioIngestError, AudioIngestPolicy};
use crate::transcription_jobs::TranscriptionJobs;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use fono_stt::ActivityGuard;
use fono_types::transcription_contract::{TranscriptionRequest, TranscriptionServiceError};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const UPLOAD_BODY_LIMIT_BYTES: usize = 101 * 1024 * 1024;
const OPENAPI_SPEC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json"));
const API_DOCUMENTATION: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/api-docs.html"));

#[derive(Clone)]
pub struct RestApiState {
    verifier: Arc<dyn TokenVerifier>,
    activity: Arc<dyn ActivityGuard>,
    pub protocol_version: u16,
    jobs: Option<Arc<dyn TranscriptionJobs>>,
    upload_dir: Arc<PathBuf>,
}

/// The desktop supplies one token; a server host can implement token-file
/// verification without changing route handlers.
pub trait TokenVerifier: Send + Sync {
    fn verify(&self, token: &str) -> bool;
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

fn authorized(headers: &HeaderMap, state: &RestApiState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| state.verifier.verify(token))
}

fn api_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(ApiError { code })).into_response()
}

fn submission_error(service_error: TranscriptionServiceError) -> axum::response::Response {
    match service_error {
        TranscriptionServiceError::InvalidRequest(_) => {
            api_error(StatusCode::BAD_REQUEST, "invalid_request")
        }
        TranscriptionServiceError::Busy(_) => {
            api_error(StatusCode::TOO_MANY_REQUESTS, "queue_full")
        }
        TranscriptionServiceError::ModelNotReady(_) => {
            api_error(StatusCode::SERVICE_UNAVAILABLE, "model_not_ready")
        }
        TranscriptionServiceError::Cancelled(_) => api_error(StatusCode::CONFLICT, "cancelled"),
        TranscriptionServiceError::Failed(_) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "transcription_failed")
        }
    }
}

async fn submit(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Json(request): Json<TranscriptionRequest>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let Some(jobs) = state.jobs.as_ref() else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match jobs.submit_job(request) {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => submission_error(error),
    }
}

async fn upload(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let _activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(_) => return api_error(StatusCode::SERVICE_UNAVAILABLE, "update_installing"),
    };
    let request = match request_from_multipart(multipart, &state.upload_dir, &state.activity).await
    {
        Ok(request) => request,
        Err(error) => return upload_error(error),
    };
    let Some(jobs) = state.jobs.as_ref() else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match jobs.submit_job(request) {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => submission_error(error),
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
    mut multipart: Multipart,
    upload_dir: &Path,
    activity: &Arc<dyn ActivityGuard>,
) -> Result<TranscriptionRequest, UploadError> {
    let mut language = "auto".to_string();
    let mut model = None;
    let mut audio_path = None;
    let result = async {
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
                    audio_path = Some(write_upload(&mut field, upload_dir, &extension).await?);
                }
                Some("audio") => {
                    return Err(UploadError::Invalid(
                        "only one audio file is allowed".into(),
                    ))
                }
                _ => return Err(UploadError::Invalid("unsupported multipart field".into())),
            }
        }
        let audio_path = audio_path
            .as_ref()
            .ok_or_else(|| UploadError::Invalid("audio field is required".into()))?;
        let decoded = tokio::task::spawn_blocking({
            let audio_path = audio_path.clone();
            let activity = activity
                .lease()
                .map_err(|_| UploadError::Internal("application update is installing".into()))?;
            move || {
                let _activity = activity;
                audio_ingest::decode_file(&audio_path, AudioIngestPolicy::default())
            }
        })
        .await
        .map_err(|error| UploadError::Internal(format!("audio decoder join failed: {error}")))?;
        let audio = decoded.map_err(UploadError::Audio)?;
        Ok(audio.into_transcription_request(language, model))
    }
    .await;
    if let Some(audio_path) = audio_path {
        let _ = tokio::fs::remove_file(audio_path).await;
    }
    result
}

async fn bounded_text(field: axum::extract::multipart::Field<'_>) -> Result<String, UploadError> {
    let text = field
        .text()
        .await
        .map_err(|error| UploadError::Invalid(error.to_string()))?;
    if text.len() > 64 || text.trim().is_empty() {
        return Err(UploadError::Invalid(
            "text fields must contain 1 through 64 characters".into(),
        ));
    }
    Ok(text)
}

async fn write_upload(
    field: &mut axum::extract::multipart::Field<'_>,
    upload_dir: &Path,
    extension: &str,
) -> Result<PathBuf, UploadError> {
    tokio::fs::create_dir_all(upload_dir)
        .await
        .map_err(|error| UploadError::Internal(error.to_string()))?;
    let path = upload_dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), extension));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|error| UploadError::Internal(error.to_string()))?;
    let mut bytes_written = 0_u64;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|error| UploadError::Invalid(error.to_string()))?
    {
        bytes_written = bytes_written.saturating_add(chunk.len() as u64);
        if bytes_written > AudioIngestPolicy::default().max_file_bytes {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(UploadError::TooLarge);
        }
        if let Err(error) = file.write_all(&chunk).await {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(UploadError::Internal(error.to_string()));
        }
    }
    if let Err(error) = file.flush().await {
        let _ = tokio::fs::remove_file(&path).await;
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
    TooLarge,
    Unsupported,
    Audio(AudioIngestError),
    Internal(String),
}

fn upload_error(error: UploadError) -> axum::response::Response {
    match error {
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
