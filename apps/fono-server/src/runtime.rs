//! Owning HTTP host for the standalone server. Disk validation precedes bind;
//! runtime readiness is published only by the real model pool.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, DirBuilder};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, Extension, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use fono_core::OperationCancellation;
use fono_service::audio_ingest::AudioIngestPolicy;
use fono_service::rest_api::{
    self, authorize_headers, RestApiState, TokenAuthorization, TokenScope, TokenVerifier,
};
use fono_service::transcription_jobs::TranscriptionJobs;
use fono_stt::{ModelPool, SttEnvironment, SttHealth};
use fono_types::{ModelId, TRANSCRIPTION_PROTOCOL_VERSION};
use serde::Serialize;
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::jobs::{acceleration_for_device, ServerActivity, ServerJobs};
use crate::models::{self, DiskState, PreloadPlan};
use crate::tokens::{FileTokenVerifier, TokenError};

const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;
const CLEANUP_ALLOWANCE: Duration = Duration::from_secs(2);
const SHUTDOWN_POLL: Duration = Duration::from_millis(25);
const MODEL_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum RuntimeError {
    Configuration(&'static str),
    Model(String),
    Tokens(TokenError),
    Bind(std::io::Error),
    Upload(std::io::Error),
    Server(std::io::Error),
    Signal(std::io::Error),
    Task(&'static str),
    ShutdownTimeout,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => formatter.write_str(message),
            Self::Model(message) => formatter.write_str(message),
            Self::Tokens(error) => write!(formatter, "Token store: {error}"),
            Self::Bind(error) => write!(formatter, "Cannot bind server listener: {error}"),
            Self::Upload(error) => write!(formatter, "Cannot prepare private uploads: {error}"),
            Self::Server(error) => write!(formatter, "HTTP server failed: {error}"),
            Self::Signal(error) => write!(formatter, "Cannot install shutdown signal: {error}"),
            Self::Task(message) => formatter.write_str(message),
            Self::ShutdownTimeout => formatter.write_str("Server shutdown exceeded its deadline"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Run until SIGTERM or SIGINT. The caller owns the Tokio runtime.
pub async fn serve(config: ServerConfig) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate()).map_err(RuntimeError::Signal)?;
        let mut interrupt = signal(SignalKind::interrupt()).map_err(RuntimeError::Signal)?;
        serve_with_shutdown(config, async move {
            tokio::select! {
                _ = terminate.recv() => {},
                _ = interrupt.recv() => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        })
        .await
    }
    #[cfg(not(unix))]
    {
        serve_with_shutdown(config, async move {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    }
}

/// The same host with an injected stop event for deterministic integration tests.
pub async fn serve_with_shutdown(
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), RuntimeError> {
    let mut shutdown = Box::pin(shutdown);
    if config.tls.is_some() {
        return Err(RuntimeError::Configuration(
            "TLS is configured but unavailable in this server build",
        ));
    }
    let max_audio_seconds = u32::try_from(config.server.max_audio_seconds).map_err(|_| {
        RuntimeError::Configuration("server.max_audio_seconds exceeds the decoder limit")
    })?;
    let max_file_bytes =
        config
            .server
            .max_upload_mb
            .checked_mul(1024 * 1024)
            .ok_or(RuntimeError::Configuration(
                "server.max_upload_mb is too large",
            ))?;
    models::validate_configuration(&config).map_err(RuntimeError::Model)?;
    let plans = required_preload(&config)?;
    for plan in &plans {
        acceleration_for_device(&plan.device).map_err(RuntimeError::Model)?;
    }
    // Opening the store before bind also checks ownership/permissions. Each
    // request still reads it again, so token revocation needs no restart.
    let verifier: Arc<dyn TokenVerifier> = Arc::new(
        FileTokenVerifier::open(config.server.tokens_file.clone()).map_err(RuntimeError::Tokens)?,
    );
    tokio::select! {
        biased;
        _ = &mut shutdown => return Ok(()),
        result = verify_required_models(&config, &plans) => result?,
    }

    let activity = ServerActivity::new();
    let environment = SttEnvironment {
        models_dir: config.models.dir.clone(),
        activity: activity.clone(),
        ..SttEnvironment::default()
    };
    let pool = Arc::new(
        ModelPool::new(environment, config.models.max_loaded)
            .map_err(|_| RuntimeError::Configuration("Cannot initialize model pool"))?,
    );
    let configured_models = configured_models(&config)?;
    let upload_dir = create_upload_dir().map_err(RuntimeError::Upload)?;
    let listener = match tokio::net::TcpListener::bind(config.server.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            let _ = cleanup_upload_dir(&upload_dir);
            return Err(RuntimeError::Bind(error));
        }
    };
    let bound = listener.local_addr().map_err(RuntimeError::Bind)?;
    if !bound.ip().is_loopback() && config.tls.is_none() {
        tracing::warn!("Fono Server is listening outside loopback without TLS");
    }

    let default_plan = plans
        .iter()
        .find(|plan| plan.id == config.models.default)
        .expect("required preload includes the default model")
        .clone();
    let jobs = Arc::new(
        match ServerJobs::new(
            Arc::clone(&pool),
            default_plan.clone(),
            &config,
            Arc::clone(&activity),
        ) {
            Ok(jobs) => jobs,
            Err(error) => {
                let _ = cleanup_upload_dir(&upload_dir);
                return Err(RuntimeError::Model(error));
            }
        },
    );
    let health = Arc::new(HealthState {
        verifier: Arc::clone(&verifier),
        pool: Arc::clone(&pool),
        jobs: jobs.jobs(),
        default_id: default_plan.selection.id.clone(),
        default_name: default_plan.id.clone(),
        models: configured_models,
        loading: Mutex::new(plans.iter().map(|plan| plan.id.clone()).collect()),
        failed: Mutex::new(HashSet::new()),
        stopping: AtomicBool::new(false),
    });
    let state = RestApiState::new("", TRANSCRIPTION_PROTOCOL_VERSION, activity.clone())
        .with_token_verifier(verifier)
        .with_jobs(jobs.jobs())
        .with_upload_dir(upload_dir.clone())
        .with_upload_policy(AudioIngestPolicy {
            max_file_bytes: max_file_bytes as u64,
            max_duration_seconds: max_audio_seconds,
        });
    let app = server_router(state, Arc::clone(&health), max_file_bytes);
    let preload_cancel = OperationCancellation::default();
    let mut preload = tokio::task::spawn_blocking({
        let pool = Arc::clone(&pool);
        let health = Arc::clone(&health);
        let cancellation = preload_cancel.clone();
        move || preload_models(pool, plans, health, cancellation)
    });
    let (maintenance_stop, mut maintenance_stop_rx) = oneshot::channel::<()>();
    let mut maintenance = tokio::spawn({
        let pool = Arc::clone(&pool);
        async move {
            let mut tick = tokio::time::interval_at(
                Instant::now() + MODEL_MAINTENANCE_INTERVAL,
                MODEL_MAINTENANCE_INTERVAL,
            );
            loop {
                tokio::select! {
                    _ = &mut maintenance_stop_rx => break,
                    _ = tick.tick() => {
                        let pool = Arc::clone(&pool);
                        if tokio::task::spawn_blocking(move || pool.reap_idle()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    let (stop_tx, stop_rx) = oneshot::channel();
    let mut http = tokio::spawn(rest_api::serve_with_shutdown(app, listener, async move {
        let _ = stop_rx.await;
    }));
    tracing::info!(address = %bound, "Fono Server listening");
    let early_server_result = tokio::select! {
        _ = &mut shutdown => None,
        result = &mut http => Some(result),
    };
    health.stopping.store(true, Ordering::Release);
    activity.close();
    jobs.cancel_pending();
    preload_cancel.cancel();
    let _ = maintenance_stop.send(());
    let _ = stop_tx.send(());

    let grace_deadline = Instant::now() + config.server.shutdown_grace;
    while jobs.running_count() > 0 && Instant::now() < grace_deadline {
        tokio::time::sleep(SHUTDOWN_POLL).await;
    }
    jobs.shutdown_worker();
    let cleanup_deadline = grace_deadline + CLEANUP_ALLOWANCE;
    let worker_join = tokio::task::spawn_blocking({
        let jobs = Arc::clone(&jobs);
        move || jobs.join_worker()
    });
    let worker_joined = tokio::time::timeout_at(cleanup_deadline, worker_join)
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|result| result.is_ok());
    // A cancelled blocking load retains its activity lease until it returns.
    let preload_joined = tokio::time::timeout_at(cleanup_deadline, &mut preload)
        .await
        .ok()
        .is_some_and(|result| result.is_ok());
    let maintenance_joined = tokio::time::timeout_at(cleanup_deadline, &mut maintenance)
        .await
        .ok()
        .is_some_and(|result| result.is_ok());

    pool.begin_shutdown();
    let pool_drained = drain_pool(&pool, cleanup_deadline).await;
    let activity_drained = if Instant::now() < cleanup_deadline {
        let remaining = cleanup_deadline - Instant::now();
        let activity = Arc::clone(&activity);
        tokio::time::timeout_at(
            cleanup_deadline,
            tokio::task::spawn_blocking(move || activity.wait_zero(remaining)),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false)
    } else {
        activity.active() == 0
    };
    let http_result = match early_server_result {
        Some(result) => result,
        None => match tokio::time::timeout_at(cleanup_deadline, &mut http).await {
            Ok(result) => result,
            Err(_) => {
                http.abort();
                return Err(RuntimeError::ShutdownTimeout);
            }
        },
    };
    if !worker_joined
        || !preload_joined
        || !maintenance_joined
        || !pool_drained
        || !activity_drained
    {
        return Err(RuntimeError::ShutdownTimeout);
    }
    let http_result = http_result
        .map_err(|_| RuntimeError::Task("HTTP server task failed"))?
        .map_err(RuntimeError::Server);
    tokio::time::timeout_at(
        cleanup_deadline,
        tokio::task::spawn_blocking(move || cleanup_upload_dir(&upload_dir)),
    )
    .await
    .map_err(|_| RuntimeError::ShutdownTimeout)?
    .map_err(|_| RuntimeError::Task("Upload cleanup task failed"))?
    .map_err(RuntimeError::Upload)?;
    http_result
}

fn create_upload_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let path = base.join(format!("fono-server-uploads-{}", Uuid::new_v4().simple()));
        #[cfg(unix)]
        let mut builder = DirBuilder::new();
        #[cfg(not(unix))]
        let builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "cannot allocate a unique private upload directory",
    ))
}

fn cleanup_upload_dir(path: &Path) -> std::io::Result<()> {
    let base = fs::canonicalize(std::env::temp_dir())?;
    let target = fs::canonicalize(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if target.parent() != Some(base.as_path())
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
    {
        return Err(std::io::Error::new(
            ErrorKind::PermissionDenied,
            "upload directory was replaced",
        ));
    }
    fs::remove_dir_all(path)
}

fn required_preload(config: &ServerConfig) -> Result<Vec<PreloadPlan>, RuntimeError> {
    let mut preload_config = config.clone();
    if !preload_config
        .models
        .preload
        .contains(&preload_config.models.default)
    {
        preload_config
            .models
            .preload
            .insert(0, preload_config.models.default.clone());
    }
    if preload_config.models.preload.len() > config.models.max_loaded {
        return Err(RuntimeError::Configuration(
            "models.default and models.preload exceed models.max_loaded",
        ));
    }
    let plans = models::resolve_preload(&preload_config).map_err(RuntimeError::Model)?;
    if plans.iter().any(|plan| {
        plan.id == config.models.default
            && !matches!(plan.residency, fono_stt::ModelResidency::Resident)
    }) {
        return Err(RuntimeError::Configuration(
            "models.default must use resident residency for readiness",
        ));
    }
    Ok(plans)
}

fn configured_models(config: &ServerConfig) -> Result<Vec<PreloadPlan>, RuntimeError> {
    let mut all = config.clone();
    all.models.preload = std::iter::once(config.models.default.clone())
        .chain(config.models.preload.iter().cloned())
        .chain(
            config
                .models
                .placement
                .iter()
                .map(|place| place.model.clone()),
        )
        .collect();
    let mut seen = HashSet::new();
    all.models.preload.retain(|id| seen.insert(id.clone()));
    models::resolve_preload(&all).map_err(RuntimeError::Model)
}

async fn verify_required_models(
    config: &ServerConfig,
    plans: &[PreloadPlan],
) -> Result<(), RuntimeError> {
    let required: HashSet<String> = plans.iter().map(|plan| plan.id.clone()).collect();
    let config = config.clone();
    let verification = tokio::task::spawn_blocking(move || models::verify(&config))
        .await
        .map_err(|_| RuntimeError::Task("Model verification task failed"))?
        .map_err(|_| RuntimeError::Configuration("Cannot verify models directory"))?;
    for id in required {
        if !verification
            .iter()
            .any(|row| row.id == id && row.state == DiskState::Verified)
        {
            return Err(RuntimeError::Model(format!(
                "Required model {id} is missing or failed checksum verification"
            )));
        }
    }
    Ok(())
}

fn preload_models(
    pool: Arc<ModelPool>,
    plans: Vec<PreloadPlan>,
    health: Arc<HealthState>,
    cancellation: OperationCancellation,
) {
    for plan in plans {
        if cancellation.is_cancelled() {
            break;
        }
        let result = acceleration_for_device(&plan.device).and_then(|mode| {
            pool.acquire(&plan.selection, mode, plan.residency, 0, &cancellation)
                .map(drop)
                .map_err(|_| "Model preload failed".to_owned())
        });
        lock(&health.loading).remove(&plan.id);
        if result.is_err() && !cancellation.is_cancelled() {
            lock(&health.failed).insert(plan.id.clone());
            tracing::error!(model_id = %plan.id, "Model preload failed");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct HealthState {
    verifier: Arc<dyn TokenVerifier>,
    pool: Arc<ModelPool>,
    jobs: Arc<dyn TranscriptionJobs>,
    default_id: ModelId,
    default_name: String,
    models: Vec<PreloadPlan>,
    loading: Mutex<HashSet<String>>,
    failed: Mutex<HashSet<String>>,
    stopping: AtomicBool,
}

#[derive(Serialize)]
struct PublicHealth {
    status: &'static str,
}

#[derive(Serialize)]
struct DetailedHealth {
    status: &'static str,
    models: Vec<HealthModel>,
    queue: HealthQueue,
}

#[derive(Serialize)]
struct HealthModel {
    id: String,
    state: &'static str,
    device: String,
}

#[derive(Serialize)]
struct HealthQueue {
    interactive: usize,
    batch: usize,
}

#[derive(Serialize)]
struct ApiError {
    error: ErrorCode,
}

#[derive(Serialize)]
struct ErrorCode {
    code: &'static str,
}

fn server_router(state: RestApiState, health: Arc<HealthState>, max_file_bytes: usize) -> Router {
    let jobs = rest_api::jobs_routes().route_layer(middleware::from_fn_with_state(
        Arc::clone(&health.verifier),
        authorize_job_request,
    ));
    let meta: Router<RestApiState> = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/health", get(detailed_health))
        .layer(Extension(health));
    jobs.merge(meta)
        .layer(DefaultBodyLimit::max(
            max_file_bytes.saturating_add(MULTIPART_OVERHEAD_BYTES),
        ))
        .with_state(state)
}

async fn authorize_job_request(
    State(verifier): State<Arc<dyn TokenVerifier>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let authorized = tokio::task::spawn_blocking(move || {
        authorize_headers(&headers, verifier.as_ref(), TokenScope::Recognize)
    })
    .await
    .unwrap_or(TokenAuthorization::Unauthorized);
    match authorized {
        TokenAuthorization::Allowed => next.run(request).await,
        TokenAuthorization::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: ErrorCode {
                    code: "unauthorized",
                },
            }),
        )
            .into_response(),
        TokenAuthorization::Forbidden => (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                error: ErrorCode { code: "forbidden" },
            }),
        )
            .into_response(),
    }
}

async fn healthz(Extension(state): Extension<Arc<HealthState>>) -> Response {
    let status = if state.stopping.load(Ordering::Acquire) {
        None
    } else {
        let pool = Arc::clone(&state.pool);
        let id = state.default_id.clone();
        tokio::task::spawn_blocking(move || pool.model_health(&id))
            .await
            .ok()
            .flatten()
    };
    let ready = matches!(
        status,
        Some(SttHealth::Ready { .. } | SttHealth::Busy { .. })
    );
    let failed = matches!(status, Some(SttHealth::Failed { .. }))
        || lock(&state.failed).contains(&state.default_name);
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(PublicHealth {
            status: if ready {
                "ok"
            } else if failed {
                "failed"
            } else {
                "starting"
            },
        }),
    )
        .into_response()
}

async fn detailed_health(
    Extension(state): Extension<Arc<HealthState>>,
    headers: HeaderMap,
) -> Response {
    let verifier = Arc::clone(&state.verifier);
    let authorization = tokio::task::spawn_blocking(move || {
        authorize_headers(&headers, verifier.as_ref(), TokenScope::Recognize)
    })
    .await
    .unwrap_or(TokenAuthorization::Unauthorized);
    match authorization {
        TokenAuthorization::Unauthorized => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiError {
                    error: ErrorCode {
                        code: "unauthorized",
                    },
                }),
            )
                .into_response()
        }
        TokenAuthorization::Forbidden => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: ErrorCode { code: "forbidden" },
                }),
            )
                .into_response()
        }
        TokenAuthorization::Allowed => {}
    }
    let pool = Arc::clone(&state.pool);
    let plans = state.models.clone();
    let loading = lock(&state.loading).clone();
    let failed = lock(&state.failed).clone();
    let statuses = tokio::task::spawn_blocking(move || {
        plans
            .into_iter()
            .map(|plan| {
                let health = pool.model_health(&plan.selection.id);
                let (state, device) = match health {
                    Some(SttHealth::Ready { device }) => ("ready", device),
                    Some(SttHealth::Busy { device }) => ("busy", device),
                    Some(SttHealth::Failed { device, .. }) => ("failed", device),
                    Some(SttHealth::Unloaded) | None if failed.contains(&plan.id) => {
                        ("failed", plan.device)
                    }
                    Some(SttHealth::Unloaded) | None if loading.contains(&plan.id) => {
                        ("loading", plan.device)
                    }
                    Some(SttHealth::Unloaded) | None => ("unloaded", plan.device),
                };
                HealthModel {
                    id: plan.id,
                    state,
                    device,
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let ready = statuses
        .iter()
        .any(|model| model.id == state.default_name && matches!(model.state, "ready" | "busy"))
        && !state.stopping.load(Ordering::Acquire);
    let failed = statuses
        .iter()
        .any(|model| model.id == state.default_name && model.state == "failed");
    let queue = state.jobs.snapshot();
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(DetailedHealth {
            status: if ready {
                "ready"
            } else if failed {
                "failed"
            } else {
                "starting"
            },
            models: statuses,
            queue: HealthQueue {
                interactive: 0,
                batch: queue.queued + queue.preparing + queue.transcribing,
            },
        }),
    )
        .into_response()
}

async fn drain_pool(pool: &Arc<ModelPool>, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        let pool_for_reap = Arc::clone(pool);
        let result = tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || pool_for_reap.reap_shutdown()),
        )
        .await;
        if result.is_err() {
            return false;
        }
        if pool.shutdown_complete() {
            return true;
        }
        tokio::time::sleep(SHUTDOWN_POLL).await;
    }
    pool.shutdown_complete()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use fono_service::transcription_jobs::{TranscriptionJob, TranscriptionQueueSnapshot};
    use fono_types::{TranscriptionRequest, TranscriptionServiceError};
    use std::ffi::OsString;
    use std::path::Path;
    use tower::ServiceExt;

    struct RecognizeVerifier;

    impl TokenVerifier for RecognizeVerifier {
        fn verify(&self, token: &str) -> bool {
            token == "test-recognize-token"
        }

        fn scope(&self, token: &str) -> Option<TokenScope> {
            self.verify(token).then_some(TokenScope::Recognize)
        }
    }

    struct StubJobs;

    impl TranscriptionJobs for StubJobs {
        fn submit_job(
            &self,
            _request: TranscriptionRequest,
        ) -> Result<TranscriptionJob, TranscriptionServiceError> {
            Err(TranscriptionServiceError::ModelNotReady(
                "not loaded".into(),
            ))
        }

        fn get_job(&self, _id: &str) -> Option<TranscriptionJob> {
            None
        }

        fn cancel_job(&self, _id: &str) -> Option<TranscriptionJob> {
            None
        }

        fn run_next_job(&self) -> Option<TranscriptionJob> {
            None
        }

        fn cancel_all_jobs(&self) {}

        fn snapshot(&self) -> TranscriptionQueueSnapshot {
            TranscriptionQueueSnapshot {
                queued: 2,
                ..TranscriptionQueueSnapshot::default()
            }
        }
    }

    fn sample_config() -> ServerConfig {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/server.toml.example");
        crate::config::load_and_validate_with_env(&path, std::iter::empty::<(OsString, OsString)>())
            .unwrap()
    }

    fn test_router() -> (Router, Arc<HealthState>) {
        let config = sample_config();
        let plan = required_preload(&config).unwrap().remove(0);
        let activity = ServerActivity::new();
        let pool = Arc::new(
            ModelPool::new(
                SttEnvironment {
                    activity: activity.clone(),
                    ..SttEnvironment::default()
                },
                1,
            )
            .unwrap(),
        );
        let verifier: Arc<dyn TokenVerifier> = Arc::new(RecognizeVerifier);
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(StubJobs);
        let health = Arc::new(HealthState {
            verifier: Arc::clone(&verifier),
            pool,
            jobs: Arc::clone(&jobs),
            default_id: plan.selection.id.clone(),
            default_name: plan.id.clone(),
            models: vec![plan.clone()],
            loading: Mutex::new(HashSet::from([plan.id])),
            failed: Mutex::new(HashSet::new()),
            stopping: AtomicBool::new(false),
        });
        let rest = RestApiState::new("", TRANSCRIPTION_PROTOCOL_VERSION, activity)
            .with_token_verifier(verifier)
            .with_jobs(jobs);
        (server_router(rest, Arc::clone(&health), 1024), health)
    }

    async fn get_json(
        app: Router,
        path: &str,
        token: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().uri(path);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn routes_distinguish_public_starting_and_authenticated_details() {
        let (app, _) = test_router();
        let (status, body) = get_json(app.clone(), "/healthz", None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "starting");

        let (status, _) = get_json(app.clone(), "/v1/health", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = get_json(app, "/v1/health", Some("test-recognize-token")).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "starting");
        assert_eq!(body["models"][0]["state"], "loading");
        assert_eq!(body["queue"]["batch"], 2);
    }

    #[tokio::test]
    async fn jobs_auth_runs_before_json_extraction() {
        let (app, _) = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/transcription-jobs")
                    .header("content-type", "application/json")
                    .body(Body::from("{invalid"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn failed_default_preload_is_never_reported_ready() {
        let (app, health) = test_router();
        lock(&health.failed).insert(health.default_name.clone());
        lock(&health.loading).remove(&health.default_name);
        let (status, body) = get_json(app, "/healthz", None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "failed");
    }

    #[test]
    fn default_is_preloaded_even_when_not_listed_explicitly() {
        let mut config = sample_config();
        config.models.preload.clear();
        let plans = required_preload(&config).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].id, config.models.default);
    }

    #[test]
    fn default_plus_preload_cannot_exceed_pool_capacity() {
        let mut config = sample_config();
        config.models.preload = vec!["whisper:small".into()];
        config.models.max_loaded = 1;
        assert!(matches!(
            required_preload(&config),
            Err(RuntimeError::Configuration(_))
        ));
    }

    #[test]
    fn default_model_cannot_unload_while_it_defines_readiness() {
        let mut config = sample_config();
        let default = config.models.default.clone();
        let placement = config
            .models
            .placement
            .iter_mut()
            .find(|placement| placement.model == default)
            .unwrap();
        placement.residency = crate::config::ModelResidency::IdleUnload;
        placement.idle_unload_after = Some(Duration::from_secs(60));
        assert!(matches!(
            required_preload(&config),
            Err(RuntimeError::Configuration(_))
        ));
    }

    #[tokio::test]
    async fn tls_config_fails_closed_before_binding() {
        let mut config = sample_config();
        config.tls = Some(crate::config::TlsSettings {
            cert: "/absent/cert.pem".into(),
            key: "/absent/key.pem".into(),
        });
        let error = serve_with_shutdown(config, std::future::pending())
            .await
            .unwrap_err();
        assert!(matches!(error, RuntimeError::Configuration(_)));
        assert!(error.to_string().contains("TLS"));
    }

    #[tokio::test]
    async fn shutdown_during_startup_does_not_wait_for_model_verification() {
        let mut config = sample_config();
        let tokens = std::env::temp_dir().join(format!("fono-startup-{}", Uuid::new_v4()));
        crate::tokens::create(&tokens, "startup-test", TokenScope::Recognize).unwrap();
        config.server.tokens_file = tokens.clone();
        let result = serve_with_shutdown(config, std::future::ready(())).await;
        std::fs::remove_file(tokens).unwrap();
        assert!(result.is_ok());
    }
}
