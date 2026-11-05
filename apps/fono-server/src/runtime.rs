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
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use fono_core::OperationCancellation;
use fono_service::audio_ingest::AudioIngestPolicy;
use fono_service::rest_api::{
    self, authorize_headers, authorize_principal_headers, RestApiState, TokenAuthorization,
    TokenScope, TokenVerifier,
};
use fono_service::transcription_jobs::TranscriptionJobs;
use fono_stt::{ModelPool, SttEnvironment, SttHealth};
use fono_types::ModelId;
use serde::Serialize;
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use crate::api::{self, ApiState};
use crate::config::LogLevel;
use crate::config::ServerConfig;
use crate::jobs::{acceleration_for_device, ServerActivity, ServerJobs};
use crate::models::{self, DiskState, PreloadPlan};
use crate::recognize::{self, RecognizeState};
use crate::telemetry::{Observability, Phase};
use crate::tokens::{FileTokenVerifier, TokenError};

const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;
const CLEANUP_ALLOWANCE: Duration = Duration::from_secs(2);
const SHUTDOWN_POLL: Duration = Duration::from_millis(25);
const MODEL_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);
/// HTTP API contract version shared by the runtime and CLI.
pub const SERVER_API_VERSION: u16 = 2;

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
    Tls(crate::tls::TlsError),
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
            Self::Tls(error) => write!(formatter, "TLS configuration: {error}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn public_message(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "Server configuration is unsupported; run doctor",
            Self::Model(_) => "Model validation or initialization failed; run doctor",
            Self::Tokens(_) => "Token store validation failed; run doctor or use the token CLI",
            Self::Bind(_) => "Cannot bind server listener; run doctor to check the configured port",
            Self::Upload(_) => {
                "Private upload storage is unavailable; check temporary directory access"
            }
            Self::Signal(_) => "Cannot install server shutdown signals",
            Self::Server(_) | Self::Task(_) => {
                "Server runtime failed; inspect the structured technical events"
            }
            Self::ShutdownTimeout => "Server shutdown exceeded its deadline",
            Self::Tls(_) => {
                "TLS configuration failed validation; run doctor before starting the server"
            }
        }
    }
    fn event_phase(&self) -> Phase {
        match self {
            Self::Configuration(_) => Phase::ConfigurationFailed,
            Self::Model(_) => Phase::ModelFailed,
            Self::Tokens(_) => Phase::TokensFailed,
            Self::Bind(_) => Phase::BindFailed,
            Self::Upload(_) => Phase::UploadFailed,
            Self::Signal(_) => Phase::SignalFailed,
            Self::ShutdownTimeout => Phase::ShutdownTimedOut,
            Self::Tls(_) => Phase::ConfigurationFailed,
            Self::Server(_) | Self::Task(_) => Phase::Failed,
        }
    }
}

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
    let (observability, writer) = Observability::stdout(config.logging.level)
        .map_err(|_| RuntimeError::Task("Cannot start JSON log writer"))?;
    observability.event(LogLevel::Info, Phase::Starting);
    let result = serve_observed(config, shutdown, observability.clone()).await;
    observability.event(
        if result.is_ok() {
            LogLevel::Info
        } else {
            LogLevel::Error
        },
        result
            .as_ref()
            .map_or_else(|error| error.event_phase(), |()| Phase::Stopped),
    );
    let _ = tokio::task::spawn_blocking(move || writer.finish()).await;
    result
}

async fn serve_observed(
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    observability: Arc<Observability>,
) -> Result<(), RuntimeError> {
    let mut shutdown = Box::pin(shutdown);
    let tls = match config.tls.clone() {
        None => None,
        Some(settings) => Some(Arc::new(
            tokio::task::spawn_blocking(move || crate::tls::TlsState::new(settings))
                .await
                .map_err(|_| RuntimeError::Tls(crate::tls::TlsError::Task))?
                .map_err(RuntimeError::Tls)?,
        )),
    };
    if !config.server.bind.ip().is_loopback()
        && (!config.server.trusted_private_network || config.server.allowed_hosts.is_empty())
    {
        return Err(RuntimeError::Configuration(
            "Non-loopback HTTP requires a trusted private network and explicit allowed hosts",
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
    let worker_config = config.clone();
    let workers = tokio::task::spawn_blocking(move || {
        crate::backends::validate_devices(&worker_config)?;
        crate::backends::worker_paths(&worker_config)
    })
    .await
    .map_err(|_| RuntimeError::Task("Worker validation task failed"))?
    .map_err(RuntimeError::Model)?;
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

    let activity = ServerActivity::with_observability(observability.clone());
    let environment = SttEnvironment {
        models_dir: config.models.dir.clone(),
        activity: activity.clone(),
        workers,
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
        observability.event(LogLevel::Warn, Phase::NetworkExposure);
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
    let queued_interactive: Arc<dyn Fn() -> usize + Send + Sync> = {
        let jobs = Arc::clone(&jobs);
        Arc::new(move || jobs.queued_interactive())
    };
    let health = Arc::new(HealthState {
        verifier: Arc::clone(&verifier),
        pool: Arc::clone(&pool),
        jobs: jobs.jobs(),
        queued_interactive,
        default_id: default_plan.selection.id.clone(),
        default_name: default_plan.id.clone(),
        models: configured_models,
        loading: Mutex::new(plans.iter().map(|plan| plan.id.clone()).collect()),
        failed: Mutex::new(HashSet::new()),
        stopping: AtomicBool::new(false),
    });
    let api_state = match ApiState::new(
        config.clone(),
        Arc::clone(&pool),
        jobs.jobs(),
        Arc::clone(&verifier),
        activity.clone(),
    ) {
        Ok(state) => state,
        Err(error) => {
            let _ = cleanup_upload_dir(&upload_dir);
            return Err(RuntimeError::Model(error));
        }
    };
    let recognize = RecognizeState::new(
        jobs.clone(),
        activity.clone(),
        upload_dir.clone(),
        AudioIngestPolicy {
            max_file_bytes: max_file_bytes as u64,
            max_duration_seconds: max_audio_seconds,
        },
        config.server.body_read_timeout,
    );
    let state = RestApiState::new("", SERVER_API_VERSION, activity.clone())
        .with_token_verifier(verifier)
        .with_jobs(jobs.jobs())
        .with_upload_dir(upload_dir.clone())
        .with_upload_policy(AudioIngestPolicy {
            max_file_bytes: max_file_bytes as u64,
            max_duration_seconds: max_audio_seconds,
        })
        .with_body_read_timeout(config.server.body_read_timeout);
    let app = server_router(
        state,
        Arc::clone(&health),
        recognize,
        max_file_bytes,
        config.server.allowed_hosts.clone(),
        api_state,
    );
    let preload_cancel = OperationCancellation::default();
    let mut preload = tokio::task::spawn_blocking({
        let pool = Arc::clone(&pool);
        let health = Arc::clone(&health);
        let cancellation = preload_cancel.clone();
        let observability = observability.clone();
        move || preload_models(pool, plans, health, cancellation, &observability)
    });
    let (maintenance_stop, mut maintenance_stop_rx) = oneshot::channel::<()>();
    let mut maintenance = tokio::spawn({
        let pool = Arc::clone(&pool);
        let observability = observability.clone();
        async move {
            let mut gpu_tick = tokio::time::interval(Duration::from_secs(5));
            let mut tick = tokio::time::interval_at(
                Instant::now() + MODEL_MAINTENANCE_INTERVAL,
                MODEL_MAINTENANCE_INTERVAL,
            );
            loop {
                tokio::select! {
                    _ = &mut maintenance_stop_rx => break,
                    _ = gpu_tick.tick(), if crate::backends::cuda_enabled() => {
                        let sampler = observability.clone();
                        if tokio::task::spawn_blocking(move || {
                            sampler.observe_gpu(crate::backends::cuda_devices().ok());
                        }).await.is_err() { observability.observe_gpu(None); }
                    }
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
    let http_activity = activity.clone();
    let mut http = tokio::spawn(async move {
        let stop = async move {
            let _ = stop_rx.await;
        };
        match tls {
            Some(tls) => crate::tls::serve(app, listener, tls, http_activity, stop).await,
            None => rest_api::serve_with_shutdown(app, listener, stop).await,
        }
    });
    observability.event(LogLevel::Info, Phase::Listening);
    let early_server_result = tokio::select! {
        _ = &mut shutdown => None,
        result = &mut http => Some(result),
    };
    health.stopping.store(true, Ordering::Release);
    observability.event(LogLevel::Info, Phase::Stopping);
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

pub(crate) fn required_preload(config: &ServerConfig) -> Result<Vec<PreloadPlan>, RuntimeError> {
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
    observability: &Observability,
) {
    for plan in plans {
        if cancellation.is_cancelled() {
            break;
        }
        let started = std::time::Instant::now();
        let result = acceleration_for_device(&plan.device).and_then(|mode| {
            pool.acquire_on_device_verified(
                &plan.selection,
                mode,
                crate::backends::cuda_index(&plan.device),
                plan.residency,
                0,
                &cancellation,
                || Ok(()),
            )
            .map(drop)
            .map_err(|_| "Model preload failed".to_owned())
        });
        observability.model_acquired(started.elapsed());
        lock(&health.loading).remove(&plan.id);
        if result.is_err() && !cancellation.is_cancelled() {
            lock(&health.failed).insert(plan.id.clone());
            observability.event(LogLevel::Error, Phase::ModelFailed);
        } else if result.is_ok() {
            observability.event(LogLevel::Info, Phase::ModelReady);
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
    queued_interactive: Arc<dyn Fn() -> usize + Send + Sync>,
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

fn server_router(
    state: RestApiState,
    health: Arc<HealthState>,
    recognize_state: Arc<RecognizeState>,
    max_file_bytes: usize,
    allowed_hosts: Vec<String>,
    api_state: Arc<ApiState>,
) -> Router {
    let jobs = rest_api::jobs_routes().route_layer(middleware::from_fn_with_state(
        Arc::clone(&health.verifier),
        authorize_job_request,
    ));
    let recognize = recognize::routes(recognize_state).route_layer(middleware::from_fn_with_state(
        Arc::clone(&health.verifier),
        authorize_job_request,
    ));
    let meta: Router<RestApiState> = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/health", get(detailed_health))
        .layer(Extension(health));
    jobs.merge(recognize)
        .merge(meta)
        .merge(api::routes(Arc::clone(&api_state)))
        .merge(crate::web::routes())
        .layer(DefaultBodyLimit::max(
            max_file_bytes.saturating_add(MULTIPART_OVERHEAD_BYTES),
        ))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            api_state.rate_limits(),
            crate::rate_limit::enforce,
        ))
        .layer(middleware::from_fn_with_state(
            Arc::new(allowed_hosts.into_iter().collect::<HashSet<_>>()),
            enforce_host,
        ))
        .layer(middleware::from_fn_with_state(
            api_state,
            api::observe_request,
        ))
        .layer(middleware::from_fn(recognize::request_id_middleware))
}

async fn enforce_host(
    State(allowed_hosts): State<Arc<HashSet<String>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(host) = request_host(&request) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: ErrorCode {
                    code: "invalid_host",
                },
            }),
        )
            .into_response();
    };
    if !allowed_hosts.contains(&host) {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            Json(ApiError {
                error: ErrorCode {
                    code: "host_not_allowed",
                },
            }),
        )
            .into_response();
    }
    next.run(request).await
}

fn request_host(request: &Request) -> Option<String> {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let header_host = match (hosts.next(), hosts.next()) {
        (Some(value), None) => Some(normalize_authority_host(value.to_str().ok()?)?),
        (None, None) => None,
        _ => return None,
    };
    let uri_host = match request.uri().authority() {
        Some(authority) => Some(normalize_authority_host(authority.as_str())?),
        None => None,
    };
    match (header_host, uri_host) {
        (Some(header), Some(uri)) if header == uri => Some(header),
        (Some(header), None) => Some(header),
        (None, Some(uri)) => Some(uri),
        _ => None,
    }
}

fn normalize_authority_host(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw
            .bytes()
            .any(|byte| byte <= b' ' || matches!(byte, b'/' | b'\\' | b'@' | b',' | b'?' | b'#'))
    {
        return None;
    }
    let host_end = if raw.starts_with('[') {
        raw.find(']')? + 1
    } else {
        raw.find(':').unwrap_or(raw.len())
    };
    let (host, suffix) = raw.split_at(host_end);
    if !suffix.is_empty() {
        let port = suffix.strip_prefix(':')?;
        if port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().ok().filter(|port| *port > 0).is_none()
        {
            return None;
        }
    }
    crate::config::normalize_host_name(host)
}

async fn authorize_job_request(
    State(verifier): State<Arc<dyn TokenVerifier>>,
    mut request: Request,
    next: Next,
) -> Response {
    let openai_endpoint = request.uri().path() == "/v1/audio/transcriptions";
    let headers = request.headers().clone();
    let authorization = tokio::task::spawn_blocking(move || {
        authorize_principal_headers(&headers, verifier.as_ref(), TokenScope::Recognize)
    })
    .await
    .unwrap_or(Err(TokenAuthorization::Unauthorized));
    match authorization {
        Ok(principal) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(TokenAuthorization::Unauthorized | TokenAuthorization::Allowed) => {
            authentication_error(StatusCode::UNAUTHORIZED, "unauthorized", openai_endpoint)
        }
        Err(TokenAuthorization::Forbidden) => {
            authentication_error(StatusCode::FORBIDDEN, "forbidden", openai_endpoint)
        }
    }
}

fn authentication_error(status: StatusCode, code: &'static str, openai_endpoint: bool) -> Response {
    if openai_endpoint {
        return (
            status,
            Json(serde_json::json!({"error": {
                "message": "A valid bearer token with the required scope is required",
                "type": "authentication_error", "param": null, "code": code
            }})),
        )
            .into_response();
    }
    (
        status,
        Json(ApiError {
            error: ErrorCode { code },
        }),
    )
        .into_response()
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
                    Some(SttHealth::Ready { device }) => (
                        "ready",
                        crate::backends::reported_device(&device, &plan.device),
                    ),
                    Some(SttHealth::Busy { device }) => (
                        "busy",
                        crate::backends::reported_device(&device, &plan.device),
                    ),
                    Some(SttHealth::Failed { device, .. }) => (
                        "failed",
                        crate::backends::reported_device(&device, &plan.device),
                    ),
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
                interactive: (state.queued_interactive)(),
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
    use crate::recognition_options::{RecognitionOptions, RecognitionOutput};
    use crate::recognize::{AdmittedRecognition, InteractiveRecognizer};
    use axum::body::{to_bytes, Body, Bytes};
    use axum::http::Request;
    use fono_service::transcription_jobs::{TranscriptionJob, TranscriptionQueueSnapshot};
    use fono_stt::scheduler::QueueClass;
    use fono_types::{
        Transcript, TranscriptionRequest, TranscriptionResult, TranscriptionServiceError,
    };
    use futures_util::stream;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

    struct ReadyRecognizer;

    impl InteractiveRecognizer for ReadyRecognizer {
        fn admit(
            self: Arc<Self>,
            _client_key: &str,
        ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
            Ok(Box::new(
                |request: TranscriptionRequest,
                 _class: QueueClass,
                 _options: RecognitionOptions,
                 _cancellation: OperationCancellation| {
                    Ok(RecognitionOutput {
                        device: Some("cpu".into()),
                        result: TranscriptionResult::from_transcript(
                            Transcript {
                                remote: None,
                                text: "test transcript".into(),
                                detected_language: Some(request.language),
                                transcribe_secs: Some(0.001),
                                audio_secs: Some(request.pcm_samples.len() as f32 / 16_000.0),
                                device: Some("CPU".into()),
                            },
                            "whisper:small".into(),
                        ),
                        words: Vec::new(),
                    })
                },
            ))
        }
    }

    struct BusyRecognizer;

    impl InteractiveRecognizer for BusyRecognizer {
        fn admit(
            self: Arc<Self>,
            _client_key: &str,
        ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
            Err(TranscriptionServiceError::Busy("test cap".into()))
        }
    }

    struct CancelRecognizer {
        started: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
    }

    impl InteractiveRecognizer for CancelRecognizer {
        fn admit(
            self: Arc<Self>,
            _client_key: &str,
        ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
            Ok(Box::new(
                move |_request: TranscriptionRequest,
                      _class: QueueClass,
                      _options: RecognitionOptions,
                      cancellation: OperationCancellation| {
                    self.started.store(true, Ordering::Release);
                    while !cancellation.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    self.cancelled.store(true, Ordering::Release);
                    Err(TranscriptionServiceError::Cancelled("disconnected".into()))
                },
            ))
        }
    }

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
        test_router_with_recognizer(Arc::new(ReadyRecognizer))
    }

    fn test_router_with_recognizer(
        recognizer: Arc<dyn InteractiveRecognizer>,
    ) -> (Router, Arc<HealthState>) {
        test_router_with_timeout(recognizer, Duration::from_secs(60))
    }

    fn test_router_with_timeout(
        recognizer: Arc<dyn InteractiveRecognizer>,
        body_read_timeout: Duration,
    ) -> (Router, Arc<HealthState>) {
        test_router_with_model_directory(recognizer, body_read_timeout, None)
    }

    fn test_router_with_model_directory(
        recognizer: Arc<dyn InteractiveRecognizer>,
        body_read_timeout: Duration,
        directory: Option<&Path>,
    ) -> (Router, Arc<HealthState>) {
        let mut config = sample_config();
        if let Some(directory) = directory {
            config.models.dir = directory.to_owned();
        }
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
            queued_interactive: Arc::new(|| 0),
            default_id: plan.selection.id.clone(),
            default_name: plan.id.clone(),
            models: vec![plan.clone()],
            loading: Mutex::new(HashSet::from([plan.id])),
            failed: Mutex::new(HashSet::new()),
            stopping: AtomicBool::new(false),
        });
        let recognize = RecognizeState::new(
            recognizer,
            activity.clone(),
            std::env::temp_dir(),
            AudioIngestPolicy {
                max_file_bytes: 4096,
                max_duration_seconds: 1,
            },
            body_read_timeout,
        );
        let api_state = ApiState::new(
            config.clone(),
            Arc::clone(&health.pool),
            Arc::clone(&jobs),
            Arc::clone(&verifier),
            activity.clone(),
        )
        .unwrap();
        let rest = RestApiState::new("", SERVER_API_VERSION, activity)
            .with_token_verifier(verifier)
            .with_jobs(jobs)
            .with_body_read_timeout(body_read_timeout);
        (
            server_router(
                rest,
                Arc::clone(&health),
                recognize,
                4096,
                config.server.allowed_hosts,
                api_state,
            ),
            health,
        )
    }

    async fn get_json(
        app: Router,
        path: &str,
        token: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().uri(path).header("host", "localhost");
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
    async fn remote_recognizer_roundtrip_busy_and_cancel_use_the_production_http_routes() {
        use fono_stt::remote::{RemoteEndpoint, RemoteErrorKind, RemoteRecognizer};
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let directory = Directory(
            std::env::temp_dir().join(format!("fono-remote-contract-{}", Uuid::new_v4())),
        );
        std::fs::create_dir(&directory.0).unwrap();
        let artifact = fono_types::WhisperArtifact::all()
            .unwrap()
            .into_iter()
            .find(|a| a.variant == "small")
            .unwrap();
        // Inventory checks metadata; no recognizer opens these sparse fixture weights.
        std::fs::File::create(directory.0.join(&artifact.filename))
            .unwrap()
            .set_len(artifact.bytes)
            .unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cases: Vec<Arc<dyn InteractiveRecognizer>> = vec![
            Arc::new(ReadyRecognizer),
            Arc::new(BusyRecognizer),
            Arc::new(CancelRecognizer {
                started: started.clone(),
                cancelled: cancelled.clone(),
            }),
        ];
        for (case, recognizer) in cases.into_iter().enumerate() {
            let (app, _) = test_router_with_model_directory(
                recognizer,
                Duration::from_secs(5),
                Some(&directory.0),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = RemoteEndpoint::new(
                &format!("http://{}/", listener.local_addr().unwrap()),
                "test-recognize-token",
            )
            .unwrap();
            let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
            let host = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stop_rx.await;
                    })
                    .await
                    .unwrap();
            });
            let id = artifact.id();
            let client = tokio::task::spawn_blocking(move || {
                RemoteRecognizer::connect(
                    endpoint,
                    id,
                    Arc::new(fono_stt::NoopActivity),
                    &OperationCancellation::default(),
                )
            })
            .await
            .unwrap()
            .unwrap();
            let cancel = OperationCancellation::default();
            let token = cancel.clone();
            let work = tokio::task::spawn_blocking(move || {
                client.transcribe(
                    &[0x1234; 160],
                    fono_stt::RecognitionOptions {
                        language: "en",
                        decoding: Default::default(),
                        glossary: &[],
                    },
                    &token,
                )
            });
            if case == 2 {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while !started.load(Ordering::Acquire) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .unwrap();
                cancel.cancel();
            }
            match case {
                0 => {
                    let result = work.await.unwrap().unwrap();
                    assert_eq!(result.text, "test transcript");
                    assert_eq!(result.device.as_deref(), Some("remote:cpu"));
                }
                1 => assert!(
                    matches!(work.await.unwrap(), Err(fono_stt::error::SttError::Remote(e)) if e.kind == RemoteErrorKind::Busy)
                ),
                _ => {
                    assert!(matches!(
                        work.await.unwrap(),
                        Err(fono_stt::error::SttError::Cancelled(_))
                    ));
                    tokio::time::timeout(Duration::from_secs(2), async {
                        while !cancelled.load(Ordering::Acquire) {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    })
                    .await
                    .unwrap();
                }
            }
            stop_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), host)
                .await
                .unwrap()
                .unwrap();
        }
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
    async fn standalone_discovery_and_documentation_are_registered_and_protected() {
        let (app, _) = test_router();
        for path in [
            "/",
            "/panel",
            "/panel/",
            "/panel/panel.js",
            "/panel/panel.css",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", "localhost")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(response.headers()["x-frame-options"], "DENY");
        }
        for path in [
            "/v1/capabilities",
            "/v1/models",
            "/openapi.json",
            "/docs",
            "/metrics",
        ] {
            let (status, _) = get_json(app.clone(), path, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
        let (status, capabilities) = get_json(
            app.clone(),
            "/v1/capabilities",
            Some("test-recognize-token"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(capabilities["api_version"], SERVER_API_VERSION);
        assert_eq!(capabilities["devices"][0]["id"], "cpu");
        assert_eq!(capabilities["features"]["openai_compatible"], true);
        assert_eq!(capabilities["features"]["client_model_load"], false);
        let (status, models) =
            get_json(app.clone(), "/v1/models", Some("test-recognize-token")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(models["object"], "list");
        let (status, spec) = get_json(app, "/openapi.json", Some("test-recognize-token")).await;
        assert_eq!(status, StatusCode::OK);
        for path in [
            "/v1/recognize",
            "/v1/models",
            "/v1/capabilities",
            "/v1/audio/transcriptions",
            "/v1/transcriptions",
            "/metrics",
        ] {
            assert!(spec["paths"].get(path).is_some(), "{path}");
        }
    }

    #[tokio::test]
    async fn openai_authentication_errors_keep_sdk_error_fields() {
        let (app, _) = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/audio/transcriptions")
                    .header("host", "localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("x-request-id"));
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(error["error"]["message"].is_string());
        assert_eq!(error["error"]["type"], "authentication_error");
    }

    #[tokio::test]
    async fn host_allowlist_rejects_missing_malformed_and_unlisted_hosts() {
        let (app, _) = test_router();
        for (host, expected) in [
            (Some("LOCALHOST:17832"), StatusCode::SERVICE_UNAVAILABLE),
            (Some("[::1]:17832"), StatusCode::SERVICE_UNAVAILABLE),
            (None, StatusCode::BAD_REQUEST),
            (
                Some("localhost.evil.example"),
                StatusCode::MISDIRECTED_REQUEST,
            ),
            (Some("localhost:"), StatusCode::BAD_REQUEST),
            (Some("localhost:0"), StatusCode::BAD_REQUEST),
            (Some("localhost:99999"), StatusCode::BAD_REQUEST),
            (Some("localhost,evil.example"), StatusCode::BAD_REQUEST),
        ] {
            let mut request = Request::builder()
                .uri("/healthz")
                .header("x-request-id", "host-check");
            if let Some(host) = host {
                request = request.header("host", host);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "Host: {host:?}");
            assert_eq!(response.headers()["x-request-id"], "host-check");
        }

        let mut request = Request::builder()
            .uri("/healthz")
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        request
            .headers_mut()
            .append(header::HOST, "evil.example".parse().unwrap());
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let request = Request::builder()
            .uri("http://evil.example/healthz")
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        assert_eq!(request_host(&request), None);
    }

    #[tokio::test]
    async fn cross_origin_preflight_does_not_receive_cors_permission() {
        let (app, _) = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/v1/recognize")
                    .header("host", "localhost")
                    .header("origin", "https://untrusted.example")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "authorization")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .is_none());
    }

    #[tokio::test]
    async fn unauthenticated_stalled_body_is_rejected_before_reading() {
        let (app, _) =
            test_router_with_timeout(Arc::new(ReadyRecognizer), Duration::from_millis(100));
        let pending_body = || Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
        for path in [
            "/v1/recognize",
            "/v1/transcriptions",
            "/v1/audio/transcriptions",
        ] {
            let response = tokio::time::timeout(
                Duration::from_secs(1),
                app.clone().oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header("host", "localhost")
                        .header("content-type", "audio/L16; rate=16000; channels=1")
                        .body(pending_body())
                        .unwrap(),
                ),
            )
            .await
            .expect("authentication must not wait for the body")
            .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn recognize_total_body_deadline_rejects_slow_drip_and_recovers() {
        let (app, _) =
            test_router_with_timeout(Arc::new(ReadyRecognizer), Duration::from_millis(100));
        let chunks_sent = Arc::new(AtomicUsize::new(0));
        let stream = stream::unfold(
            (0usize, Arc::clone(&chunks_sent)),
            |(index, sent)| async move {
                if index == 8 {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
                sent.fetch_add(1, Ordering::Relaxed);
                Some((
                    Ok::<_, std::io::Error>(Bytes::from_static(&[0])),
                    (index + 1, sent),
                ))
            },
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/recognize")
                    .header("host", "localhost")
                    .header("Authorization", "Bearer test-recognize-token")
                    .header("content-type", "audio/L16; rate=16000; channels=1")
                    .header("x-request-id", "slow-body")
                    .body(Body::from_stream(stream))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(response.headers()["x-request-id"], "slow-body");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "body_read_timeout");
        assert!(chunks_sent.load(Ordering::Relaxed) >= 2);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/recognize")
                    .header("host", "localhost")
                    .header("Authorization", "Bearer test-recognize-token")
                    .header("content-type", "audio/L16; rate=16000; channels=1")
                    .body(Body::from(vec![1, 0, 2, 0]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn jobs_auth_runs_before_json_extraction() {
        let (app, _) = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/transcription-jobs")
                    .header("host", "localhost")
                    .header("content-type", "application/json")
                    .body(Body::from("{invalid"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn recognize_auth_precedes_audio_extraction_and_returns_request_id() {
        let (app, _) = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/recognize")
                    .header("host", "localhost")
                    .header("content-type", "audio/unsupported")
                    .header("x-request-id", "test-unauthorized")
                    .body(Body::from("invalid audio"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["x-request-id"], "test-unauthorized");
    }

    #[tokio::test]
    async fn recognize_returns_api_v2_shape_and_busy_retry_after() {
        let request = || {
            Request::builder()
                .method("POST")
                .uri("/v1/recognize?language=ru&priority=interactive")
                .header("host", "localhost")
                .header("Authorization", "Bearer test-recognize-token")
                .header("content-type", "audio/L16; rate=16000; channels=1")
                .header("x-request-id", "sample-recognize")
                .body(Body::from(vec![1, 0, 2, 0]))
                .unwrap()
        };
        let (app, _) = test_router();
        let response = app.oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-request-id"], "sample-recognize");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["text"], "test transcript");
        assert_eq!(json["language"], "ru");
        assert_eq!(json["model"], "whisper:small");
        assert_eq!(json["backend"], "cpu");
        assert_eq!(json["device"], "cpu");
        assert_eq!(json["request_id"], "sample-recognize");
        assert!(json["timings"]["queue_ms"].is_null());

        let (busy_app, _) = test_router_with_recognizer(Arc::new(BusyRecognizer));
        let response = busy_app.oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "1");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "busy");
        assert_eq!(json["error"]["request_id"], "sample-recognize");
    }

    #[tokio::test]
    async fn recognize_accepts_wav_and_rejects_oversized_audio() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/fono-service/tests/fixtures/audio/tone-16k-mono.wav");
        let body = fs::read(path).unwrap();
        let (app, _) = test_router();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/recognize?language=en")
                    .header("host", "localhost")
                    .header("Authorization", "Bearer test-recognize-token")
                    .header("content-type", "audio/wav")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/recognize")
                    .header("host", "localhost")
                    .header("Authorization", "Bearer test-recognize-token")
                    .header("content-type", "audio/L16;rate=16000;channels=1")
                    .body(Body::from(vec![0u8; 4097]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "payload_too_large");
    }

    #[tokio::test]
    async fn recognize_disconnect_cancels_native_work() {
        let started = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let recognizer = Arc::new(CancelRecognizer {
            started: Arc::clone(&started),
            cancelled: Arc::clone(&cancelled),
        });
        let (app, _) = test_router_with_recognizer(recognizer);
        let request = Request::builder()
            .method("POST")
            .uri("/v1/recognize")
            .header("host", "localhost")
            .header("Authorization", "Bearer test-recognize-token")
            .header("content-type", "audio/L16; rate=16000; channels=1")
            .body(Body::from(vec![1, 0, 2, 0]))
            .unwrap();
        let pending = tokio::spawn(async move { app.oneshot(request).await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !started.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        pending.abort();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !cancelled.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
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
        assert!(matches!(error, RuntimeError::Tls(_)));
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
