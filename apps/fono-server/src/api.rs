//! Discovery, administration and observability for the standalone API.
//! Filesystem inventory and native model work always stay off the HTTP thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use fono_core::OperationCancellation;
use fono_service::rest_api::{
    authorize_headers, RestApiState, TokenAuthorization, TokenScope, TokenVerifier,
};
use fono_service::transcription_jobs::TranscriptionJobs;
use fono_stt::error::SttError;
use fono_stt::{ActivityGuard, ModelPool, SttHealth};
use fono_types::{CoreError, ErrorCode, ModelId, SttEngine, WhisperArtifact};
use serde::Serialize;

use crate::config::ServerConfig;
use crate::jobs::{acceleration_for_device, ServerActivity};
use crate::models::{self, DiskState};
use crate::runtime::SERVER_API_VERSION;

const OPENAPI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json"));
const DOCUMENTATION: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/api-docs.html"));

#[derive(Default)]
struct RequestMetrics {
    requests: AtomicU64,
    errors: AtomicU64,
    inflight: AtomicU64,
}

pub struct ApiState {
    config: ServerConfig,
    pool: Arc<ModelPool>,
    jobs: Arc<dyn TranscriptionJobs>,
    verifier: Arc<dyn TokenVerifier>,
    activity: Arc<ServerActivity>,
    requests: RequestMetrics,
    rate: Arc<crate::rate_limit::RateState>,
}

impl ApiState {
    /// Construction is side effect free. Handlers inspect disk asynchronously.
    pub fn new(
        config: ServerConfig,
        pool: Arc<ModelPool>,
        jobs: Arc<dyn TranscriptionJobs>,
        verifier: Arc<dyn TokenVerifier>,
        activity: Arc<ServerActivity>,
    ) -> Result<Arc<Self>, String> {
        models::validate_configuration(&config)?;
        Ok(Arc::new(Self {
            rate: Arc::new(crate::rate_limit::RateState {
                limiter: crate::rate_limit::RateLimiter::new(config.rate_limit.clone()),
                verifier: verifier.clone(),
            }),
            config,
            pool,
            jobs,
            verifier,
            activity,
            requests: RequestMetrics::default(),
        }))
    }

    pub fn rate_limits(&self) -> Arc<crate::rate_limit::RateState> {
        self.rate.clone()
    }
}

pub fn routes(state: Arc<ApiState>) -> Router<RestApiState> {
    Router::new()
        .route("/v1/clients", get(clients::list).post(clients::create))
        .route("/v1/clients/:name", axum::routing::delete(clients::revoke))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/models", get(list_models))
        .route("/v1/models/:id/load", post(load_model))
        .route("/v1/models/:id/unload", post(unload_model))
        .route("/metrics", get(metrics))
        .route("/openapi.json", get(openapi))
        .route("/docs", get(documentation))
        .layer(Extension(state))
}

/// The host applies this to the full router, including rejected requests.
/// Counter labels intentionally exclude URLs, model names and token identities.
pub async fn observe_request(
    State(state): State<Arc<ApiState>>,
    request: Request,
    next: Next,
) -> Response {
    struct Inflight(Arc<ApiState>);
    impl Drop for Inflight {
        fn drop(&mut self) {
            self.0.requests.inflight.fetch_sub(1, Ordering::Relaxed);
        }
    }
    state.requests.requests.fetch_add(1, Ordering::Relaxed);
    state.requests.inflight.fetch_add(1, Ordering::Relaxed);
    let _inflight = Inflight(Arc::clone(&state));
    let started = Instant::now();
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    state
        .activity
        .observability
        .http_completed(response.status().as_u16(), started.elapsed());
    if response.status().is_client_error() || response.status().is_server_error() {
        state.requests.errors.fetch_add(1, Ordering::Relaxed);
    }
    response
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}
#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: String,
}

struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
}
impl ApiError {
    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: "Server operation failed",
            retryable: false,
        }
    }
    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "model_not_found",
            message: "The selected model is not installed",
            retryable: false,
        }
    }
    fn response(self, headers: &HeaderMap) -> Response {
        let request_id = headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 128
                    && id.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut response = (
            self.status,
            Json(ErrorBody {
                error: ErrorDetail {
                    code: self.code,
                    message: self.message,
                    retryable: self.retryable,
                    request_id: request_id.clone(),
                },
            }),
        )
            .into_response();
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("safe request id"),
        );
        if self.retryable {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}

async fn authorize(
    state: &ApiState,
    headers: &HeaderMap,
    scope: TokenScope,
) -> Result<(), ApiError> {
    let verifier = Arc::clone(&state.verifier);
    let headers = headers.clone();
    let authorization =
        tokio::task::spawn_blocking(move || authorize_headers(&headers, verifier.as_ref(), scope))
            .await
            .map_err(|_| ApiError::internal())?;
    match authorization {
        TokenAuthorization::Allowed => Ok(()),
        TokenAuthorization::Unauthorized => Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid bearer token is required",
            retryable: false,
        }),
        TokenAuthorization::Forbidden => Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: "An admin token is required",
            retryable: false,
        }),
    }
}

#[derive(Serialize)]
struct Capabilities {
    product: &'static str,
    version: &'static str,
    api_version: u16,
    worker_protocol: u16,
    audio_formats: [&'static str; 3],
    max_audio_seconds: u64,
    max_upload_bytes: u64,
    body_read_timeout_seconds: u64,
    devices: Vec<Device>,
    features: Features,
}
#[derive(Serialize)]
struct Device {
    id: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    threads: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_total_mb: Option<u64>,
}
#[derive(Serialize)]
struct Features {
    cancel_on_disconnect: bool,
    glossary: bool,
    timestamps: [&'static str; 3],
    decoding: [&'static str; 2],
    openai_compatible: bool,
    jobs: bool,
    client_model_load: bool,
    metrics: bool,
}

async fn capabilities(Extension(state): Extension<Arc<ApiState>>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Recognize).await {
        return error.response(&headers);
    }
    let mut devices = vec![Device {
        id: "cpu".into(),
        kind: "cpu",
        threads: Some(std::thread::available_parallelism().map_or(1, usize::from)),
        name: None,
        memory_total_mb: None,
    }];
    if crate::backends::cuda_enabled() {
        let observed = tokio::task::spawn_blocking(|| {
            crate::backends::installed_worker()?;
            crate::backends::cuda_devices()
        })
        .await;
        if let Ok(Ok(gpus)) = observed {
            devices.extend(gpus.into_iter().map(|gpu| Device {
                id: format!("cuda:{}", gpu.index),
                kind: "cuda",
                threads: None,
                name: Some(gpu.name),
                memory_total_mb: Some(gpu.memory_total_mb),
            }));
        }
    }
    Json(Capabilities {
        product: "fono-server",
        version: env!("FONO_PRODUCT_VERSION"),
        api_version: SERVER_API_VERSION,
        worker_protocol: fono_stt_protocol::PROTOCOL_VERSION,
        audio_formats: [
            "audio/L16;rate=16000;channels=1",
            "audio/wav",
            "audio/ogg;codecs=opus",
        ],
        max_audio_seconds: state.config.server.max_audio_seconds,
        max_upload_bytes: (state.config.server.max_upload_mb as u64).saturating_mul(1024 * 1024),
        body_read_timeout_seconds: state.config.server.body_read_timeout.as_secs(),
        devices,
        features: Features {
            cancel_on_disconnect: true,
            glossary: true,
            timestamps: ["none", "segments", "words"],
            decoding: ["fast", "accurate"],
            openai_compatible: true,
            jobs: true,
            client_model_load: state.config.models.client_model_load,
            metrics: state.config.metrics.enabled,
        },
    })
    .into_response()
}

#[derive(Serialize)]
struct ModelList {
    object: &'static str,
    data: Vec<ModelDescription>,
}
#[derive(Serialize)]
struct ModelDescription {
    id: String,
    object: &'static str,
    fono: ModelDetails,
}
#[derive(Serialize)]
struct ModelDetails {
    engine: SttEngine,
    languages: Vec<String>,
    punctuation: bool,
    size_bytes: u64,
    state: &'static str,
    disk_state: &'static str,
    device: String,
    default: bool,
    features: ModelFeatures,
}
#[derive(Serialize)]
struct ModelFeatures {
    glossary: bool,
    prompt: bool,
    decoding: Vec<&'static str>,
    timestamps: [&'static str; 3],
}

fn canonical_id(id: &ModelId) -> String {
    if id.engine == SttEngine::Whisper {
        format!("whisper:{}", id.variant)
    } else {
        id.public_id()
    }
}

fn inventory(state: &ApiState) -> Result<ModelList, ApiError> {
    let disk: HashMap<_, _> = models::list(&state.config)
        .map_err(|_| ApiError::internal())?
        .into_iter()
        .map(|row| (row.id.clone(), row))
        .collect();
    let mut descriptions = Vec::new();
    let mut catalog = Vec::new();
    for artifact in WhisperArtifact::all().map_err(|_| ApiError::internal())? {
        catalog.push((
            artifact.id(),
            vec!["auto".to_owned(), "ru".to_owned(), "en".to_owned()],
            true,
        ));
    }
    for spec in fono_stt::offline_models::specifications().map_err(|_| ApiError::internal())? {
        let mut languages = spec.languages;
        if !languages.iter().any(|language| language == "auto") {
            languages.insert(0, "auto".to_owned());
        }
        catalog.push((spec.id, languages, spec.punctuation));
    }
    for (id, languages, punctuation) in catalog {
        let public_id = canonical_id(&id);
        let Some(row) = disk.get(&public_id) else {
            continue;
        };
        let health = state.pool.model_health(&id);
        let reservation = state.pool.model_reservation_state(&id);
        if row.state == DiskState::Missing && reservation.is_none() {
            continue;
        }
        let configured_device = state
            .config
            .models
            .placement
            .iter()
            .find(|placement| placement.model == public_id)
            .map_or("cpu", |placement| placement.device.as_str());
        let (model_state, device) = match health {
            Some(SttHealth::Ready { device }) => (
                "ready",
                crate::backends::reported_device(&device, configured_device),
            ),
            Some(SttHealth::Busy { device }) => (
                "busy",
                crate::backends::reported_device(&device, configured_device),
            ),
            Some(SttHealth::Failed { device, .. }) => (
                "failed",
                crate::backends::reported_device(&device, configured_device),
            ),
            Some(SttHealth::Unloaded) | None => (
                match reservation {
                    Some("loading") => "loading",
                    Some("retiring") => "retiring",
                    _ if row.state == DiskState::Invalid => "failed",
                    _ => "unloaded",
                },
                configured_device.to_owned(),
            ),
        };
        let whisper = id.engine == SttEngine::Whisper;
        descriptions.push(ModelDescription {
            id: public_id.clone(),
            object: "model",
            fono: ModelDetails {
                engine: id.engine,
                languages,
                punctuation,
                size_bytes: row.bytes,
                state: model_state,
                disk_state: row.state.as_str(),
                device,
                default: public_id == state.config.models.default,
                features: ModelFeatures {
                    glossary: whisper,
                    prompt: whisper,
                    decoding: if whisper {
                        vec!["fast", "accurate"]
                    } else {
                        vec!["fast"]
                    },
                    timestamps: ["none", "segments", "words"],
                },
            },
        });
    }
    descriptions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ModelList {
        object: "list",
        data: descriptions,
    })
}

async fn list_models(Extension(state): Extension<Arc<ApiState>>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Recognize).await {
        return error.response(&headers);
    }
    match tokio::task::spawn_blocking(move || inventory(&state)).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(error)) => error.response(&headers),
        Err(_) => ApiError::internal().response(&headers),
    }
}

async fn load_model(
    Extension(state): Extension<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    model_command(state, headers, id, true).await
}
async fn unload_model(
    Extension(state): Extension<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    model_command(state, headers, id, false).await
}

#[derive(Serialize)]
struct ModelCommand {
    id: String,
    state: &'static str,
}

fn command_error(error: SttError) -> ApiError {
    match CoreError::from(&error).code {
        ErrorCode::Busy => ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "busy",
            message: "The model is in use or capacity is unavailable",
            retryable: true,
        },
        ErrorCode::Cancelled => ApiError {
            status: StatusCode::REQUEST_TIMEOUT,
            code: "cancelled",
            message: "Model operation was cancelled",
            retryable: false,
        },
        ErrorCode::ModelNotFound => ApiError::not_found(),
        ErrorCode::ModelNotReady => ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "model_not_ready",
            message: "The model is not ready",
            retryable: true,
        },
        ErrorCode::InvalidRequest => ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "model_invalid",
            message: "The installed model failed verification",
            retryable: false,
        },
        _ => ApiError::internal(),
    }
}

async fn model_command(
    state: Arc<ApiState>,
    headers: HeaderMap,
    id: String,
    load: bool,
) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Admin).await {
        return error.response(&headers);
    }
    if !state.config.models.client_model_load {
        return ApiError {
            status: StatusCode::FORBIDDEN,
            code: "model_control_disabled",
            message: "Client model control is disabled",
            retryable: false,
        }
        .response(&headers);
    }
    if !load && id == state.config.models.default {
        return ApiError {
            status: StatusCode::CONFLICT,
            code: "default_model_resident",
            message: "The default model must remain loaded for readiness",
            retryable: false,
        }
        .response(&headers);
    }
    let activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(error) => return command_error(error).response(&headers),
    };
    let cancellation = OperationCancellation::default();
    struct CancelOnDrop(OperationCancellation);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let _cancellation = CancelOnDrop(cancellation.clone());
    let result = tokio::task::spawn_blocking(move || {
        let _activity = activity;
        let listing = models::list(&state.config).map_err(|_| ApiError::internal())?;
        let row = listing
            .iter()
            .find(|row| row.id == id)
            .ok_or_else(ApiError::not_found)?;
        if load && row.state == DiskState::Missing {
            return Err(ApiError::not_found());
        }
        if !load {
            let mut resolution = state.config.clone();
            resolution.models.preload = vec![id.clone()];
            let plan = models::resolve_preload(&resolution)
                .map_err(|_| ApiError::not_found())?
                .remove(0);
            state
                .pool
                .unload(&plan.selection.id)
                .map_err(command_error)?;
            let model_state =
                if state.pool.model_reservation_state(&plan.selection.id) == Some("retiring") {
                    "retiring"
                } else {
                    "unloaded"
                };
            return Ok(ModelCommand {
                id,
                state: model_state,
            });
        }
        let plan = models::resolve_installed(&state.config, &id).map_err(|_| ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "model_invalid",
            message: "The installed model failed verification",
            retryable: false,
        })?;
        let acceleration = acceleration_for_device(&plan.device).map_err(|_| ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "backend_unavailable",
            message: "The configured backend is unavailable",
            retryable: false,
        })?;
        state
            .pool
            .acquire_on_device_verified(
                &plan.selection,
                acceleration,
                crate::backends::cuda_index(&plan.device),
                plan.residency,
                0,
                &cancellation,
                || {
                    models::resolve_installed(&state.config, &id)
                        .map(|_| ())
                        .map_err(SttError::Config)
                },
            )
            .map_err(command_error)?;
        Ok(ModelCommand { id, state: "ready" })
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(error)) => error.response(&headers),
        Err(_) => ApiError::internal().response(&headers),
    }
}

async fn metrics(Extension(state): Extension<Arc<ApiState>>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Admin).await {
        return error.response(&headers);
    }
    if !state.config.metrics.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    let pool = state.pool.status();
    let queue = state.jobs.snapshot();
    let counters = &state.requests;
    let mut body = format!(
        "# TYPE fono_http_requests_total counter\nfono_http_requests_total {}\n# TYPE fono_http_errors_total counter\nfono_http_errors_total {}\n# TYPE fono_http_inflight gauge\nfono_http_inflight {}\n# TYPE fono_models gauge\nfono_models{{state=\"loaded\"}} {}\nfono_models{{state=\"loading\"}} {}\nfono_models{{state=\"retiring\"}} {}\n# TYPE fono_jobs gauge\nfono_jobs{{state=\"queued\"}} {}\nfono_jobs{{state=\"preparing\"}} {}\nfono_jobs{{state=\"transcribing\"}} {}\nfono_jobs{{state=\"completed\"}} {}\nfono_jobs{{state=\"failed\"}} {}\nfono_jobs{{state=\"cancelled\"}} {}\n# TYPE fono_activity gauge\nfono_activity {}\n",
        counters.requests.load(Ordering::Relaxed), counters.errors.load(Ordering::Relaxed), counters.inflight.load(Ordering::Relaxed), pool.loaded, pool.loading, pool.retiring, queue.queued, queue.preparing, queue.transcribing, queue.completed, queue.failed, queue.cancelled, state.activity.active(),
    );
    state.activity.observability.render(&mut body);
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

async fn openapi(Extension(state): Extension<Arc<ApiState>>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Recognize).await {
        return error.response(&headers);
    }
    ([(header::CONTENT_TYPE, "application/json")], OPENAPI).into_response()
}
async fn documentation(Extension(state): Extension<Arc<ApiState>>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Recognize).await {
        return error.response(&headers);
    }
    crate::web::response(DOCUMENTATION.as_bytes(), "text/html; charset=utf-8")
}

#[cfg(test)]
mod tests;

mod clients;
