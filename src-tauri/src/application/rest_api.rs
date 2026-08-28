//! Loopback-only HTTP surface for the local transcription service.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::application::transcription_contract::TranscriptionRequest;
use crate::application::transcription_jobs::TranscriptionJobs;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::sync::oneshot;

use crate::application::transcription_contract::TranscriptionServiceError;

#[derive(Clone)]
pub struct RestApiState {
    token: Arc<str>,
    pub protocol_version: u16,
    jobs: Option<Arc<dyn TranscriptionJobs>>,
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
        }
    }

    pub fn with_jobs(mut self, jobs: Arc<dyn TranscriptionJobs>) -> Self {
        self.jobs = Some(jobs);
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
        .route("/v1/transcription-jobs/:id", get(job))
        .route("/v1/transcription-jobs/:id/cancel", post(cancel))
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

async fn job(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    Path(id): Path<String>,
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
