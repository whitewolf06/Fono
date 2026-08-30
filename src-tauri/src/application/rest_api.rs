//! Loopback-only HTTP surface for the local transcription service.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::audio_ingest::{self, AudioIngestError, AudioIngestPolicy};
use crate::application::transcription_contract::{TranscriptionRequest, TranscriptionServiceError};
use crate::application::transcription_jobs::TranscriptionJobs;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

const UPLOAD_BODY_LIMIT_BYTES: usize = 101 * 1024 * 1024;

#[derive(Clone)]
pub struct RestApiState {
    token: Arc<str>,
    pub protocol_version: u16,
    jobs: Option<Arc<dyn TranscriptionJobs>>,
    upload_dir: Arc<PathBuf>,
}

/// Running loopback server. The transport owns a shutdown channel so Tauri can
/// stop accepting local requests before its STT runtime shuts down.
pub struct RestApiServer {
    address: SocketAddr,
    shutdown: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl RestApiServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub fn shutdown(&self) {
        if let Some(sender) = self
            .shutdown
            .lock()
            .expect("REST shutdown mutex poisoned")
            .take()
        {
            let _ = sender.send(());
        }
    }
}

impl Drop for RestApiServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl RestApiState {
    pub fn new(token: impl Into<Arc<str>>, protocol_version: u16) -> Self {
        Self {
            token: token.into(),
            protocol_version,
            jobs: None,
            upload_dir: Arc::new(std::env::temp_dir().join("fono-transcription-uploads")),
        }
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
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/transcription-jobs", post(submit))
        .route("/v1/transcriptions", post(upload))
        .route("/v1/transcription-jobs/:id", get(job))
        .route("/v1/transcription-jobs/:id/cancel", post(cancel))
        .layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT_BYTES))
        .with_state(state)
}

/// Starts only on the IPv4 loopback interface. Passing port zero asks the OS
/// for an available ephemeral port, which is useful for tests and diagnostics.
pub async fn start(state: RestApiState, port: u16) -> std::io::Result<RestApiServer> {
    let listener = tokio::net::TcpListener::bind(loopback_addr(port)).await?;
    let address = listener.local_addr()?;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();

    tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, router(state))
            .with_graceful_shutdown(async move {
                let _ = shutdown_receiver.await;
            })
            .await
        {
            tracing::error!(%error, "local transcription REST server stopped unexpectedly");
        }
    });

    Ok(RestApiServer {
        address,
        shutdown: std::sync::Mutex::new(Some(shutdown_sender)),
    })
}

fn authorized(headers: &HeaderMap, state: &RestApiState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        == Some(state.token.as_ref())
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
    let request = match request_from_multipart(multipart, &state.upload_dir).await {
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
        let model = model.ok_or_else(|| UploadError::Invalid("model field is required".into()))?;
        let audio_path = audio_path
            .as_ref()
            .ok_or_else(|| UploadError::Invalid("audio field is required".into()))?;
        let decoded = tokio::task::spawn_blocking({
            let audio_path = audio_path.clone();
            move || audio_ingest::decode_file(&audio_path, AudioIngestPolicy::default())
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
            tracing::warn!(%message, "invalid local transcription upload");
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
        UploadError::Audio(AudioIngestError::Io(message)) | UploadError::Internal(message) => {
            tracing::error!(%message, "local transcription upload failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "upload_failed")
        }
    }
}

pub fn loopback_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::application::transcription_jobs::TranscriptionJob;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    struct StubJobs {
        job: Mutex<TranscriptionJob>,
        submit_error: Option<TranscriptionServiceError>,
    }

    impl StubJobs {
        fn ready() -> Self {
            Self {
                job: Mutex::new(TranscriptionJob {
                    id: "tr_0000000000000001".into(),
                    state: crate::application::transcription_jobs::JobState::Queued,
                    result: None,
                    error: None,
                }),
                submit_error: None,
            }
        }

        fn busy() -> Self {
            Self {
                job: Mutex::new(TranscriptionJob {
                    id: "unused".into(),
                    state: crate::application::transcription_jobs::JobState::Queued,
                    result: None,
                    error: None,
                }),
                submit_error: Some(TranscriptionServiceError::Busy("full".into())),
            }
        }
    }

    impl TranscriptionJobs for StubJobs {
        fn submit_job(
            &self,
            _: TranscriptionRequest,
        ) -> Result<TranscriptionJob, TranscriptionServiceError> {
            match &self.submit_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.job.lock().unwrap().clone()),
            }
        }

        fn get_job(&self, id: &str) -> Option<TranscriptionJob> {
            let job = self.job.lock().unwrap();
            (job.id == id).then(|| job.clone())
        }

        fn cancel_job(&self, id: &str) -> Option<TranscriptionJob> {
            let mut job = self.job.lock().unwrap();
            if job.id != id {
                return None;
            }
            job.state = crate::application::transcription_jobs::JobState::Cancelled;
            Some(job.clone())
        }

        fn run_next_job(&self) -> Option<TranscriptionJob> {
            None
        }

        fn cancel_all_jobs(&self) {}
    }

    fn authorized_request(uri: &str) -> axum::http::request::Builder {
        Request::builder()
            .uri(uri)
            .header(header::AUTHORIZATION, "Bearer test-token")
    }

    #[test]
    fn always_binds_to_ipv4_loopback() {
        assert_eq!(loopback_addr(0).ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[tokio::test]
    async fn health_requires_the_bearer_token() {
        let app = router(RestApiState::new("test-token", 1));
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .header(header::AUTHORIZATION, "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn jobs_support_submit_lookup_and_cancellation() {
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(StubJobs::ready());
        let app = router(RestApiState::new("test-token", 1).with_jobs(jobs));
        let request = TranscriptionRequest {
            pcm_samples: vec![1, -1],
            language: "ru".into(),
            model: "base".into(),
        };
        let submitted = app
            .clone()
            .oneshot(
                authorized_request("/v1/transcription-jobs")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(submitted.status(), StatusCode::ACCEPTED);

        let found = app
            .clone()
            .oneshot(
                authorized_request("/v1/transcription-jobs/tr_0000000000000001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(found.status(), StatusCode::OK);

        let cancelled = app
            .oneshot(
                authorized_request("/v1/transcription-jobs/tr_0000000000000001/cancel")
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancelled.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_full_queue_returns_retryable_client_status() {
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(StubJobs::busy());
        let app = router(RestApiState::new("test-token", 1).with_jobs(jobs));
        let request = TranscriptionRequest {
            pcm_samples: vec![1],
            language: "auto".into(),
            model: "base".into(),
        };

        let response = app
            .oneshot(
                authorized_request("/v1/transcription-jobs")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn multipart_wav_becomes_a_queued_job_and_cleans_up_upload() {
        let upload_dir =
            std::env::temp_dir().join(format!("fono-rest-test-{}", uuid::Uuid::new_v4()));
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(StubJobs::ready());
        let app = router(
            RestApiState::new("test-token", 1)
                .with_jobs(jobs)
                .with_upload_dir(upload_dir.clone()),
        );
        let boundary = "fono-test-boundary";
        let mut body = Vec::new();
        multipart_text(&mut body, boundary, "language", "ru");
        multipart_text(&mut body, boundary, "model", "base");
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"sample.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/whisper.cpp/bindings/go/samples/jfk.wav"
        )));
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let response = app
            .oneshot(
                authorized_request("/v1/transcriptions")
                    .method("POST")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(std::fs::read_dir(&upload_dir).unwrap().next().is_none());
        std::fs::remove_dir_all(upload_dir).unwrap();
    }

    fn multipart_text(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }

    #[tokio::test]
    async fn started_server_is_loopback_only_and_stops_cleanly() {
        let server = start(RestApiState::new("test-token", 1), 0)
            .await
            .expect("start loopback server");
        assert_eq!(server.local_addr().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));

        let response = reqwest::Client::new()
            .get(format!("http://{}/v1/health", server.local_addr()))
            .bearer_auth("test-token")
            .send()
            .await
            .expect("call health endpoint");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        server.shutdown();
        server.shutdown();
    }
}
