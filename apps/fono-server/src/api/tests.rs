use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use axum::middleware;
use fono_service::transcription_jobs::{TranscriptionJob, TranscriptionQueueSnapshot};
use fono_stt::SttEnvironment;
use fono_types::{TranscriptionRequest, TranscriptionServiceError};
use std::ffi::OsString;
use std::path::{Path as FilePath, PathBuf};
use std::time::Duration;
use tower::ServiceExt;

#[path = "client_tests.rs"]
mod clients;

struct Tokens;
impl TokenVerifier for Tokens {
    fn verify(&self, token: &str) -> bool {
        matches!(token, "recognize-test" | "admin-test")
    }
    fn scope(&self, token: &str) -> Option<TokenScope> {
        match token {
            "recognize-test" => Some(TokenScope::Recognize),
            "admin-test" => Some(TokenScope::Admin),
            _ => None,
        }
    }
}
struct Jobs;
impl TranscriptionJobs for Jobs {
    fn submit_job(
        &self,
        _: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        unreachable!()
    }
    fn get_job(&self, _: &str) -> Option<TranscriptionJob> {
        None
    }
    fn cancel_job(&self, _: &str) -> Option<TranscriptionJob> {
        None
    }
    fn run_next_job(&self) -> Option<TranscriptionJob> {
        None
    }
    fn cancel_all_jobs(&self) {}
    fn snapshot(&self) -> TranscriptionQueueSnapshot {
        TranscriptionQueueSnapshot {
            queued: 2,
            completed: 3,
            ..Default::default()
        }
    }
}
struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_state(control: bool, metrics_enabled: bool) -> (Arc<ApiState>, TestDirectory) {
    test_state_with_activity(control, metrics_enabled, ServerActivity::new())
}

fn test_state_with_activity(
    control: bool,
    metrics_enabled: bool,
    activity: Arc<ServerActivity>,
) -> (Arc<ApiState>, TestDirectory) {
    let config_path =
        FilePath::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/server.toml.example");
    let mut config = crate::config::load_and_validate_with_env(
        &config_path,
        std::iter::empty::<(OsString, OsString)>(),
    )
    .unwrap();
    let directory = TestDirectory::new();
    config.models.dir = directory.0.clone();
    config.models.client_model_load = control;
    config.metrics.enabled = metrics_enabled;
    let pool = Arc::new(
        ModelPool::new(
            SttEnvironment {
                activity: activity.clone(),
                models_dir: directory.0.clone(),
                ..SttEnvironment::default()
            },
            2,
        )
        .unwrap(),
    );
    let state = ApiState::new(config, pool, Arc::new(Jobs), Arc::new(Tokens), activity).unwrap();
    (state, directory)
}

#[tokio::test]
async fn http_telemetry_never_copies_untrusted_headers_query_or_request_id() {
    use std::sync::Mutex;
    #[derive(Clone)]
    struct Output(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let output = Arc::new(Mutex::new(Vec::new()));
    let (observer, guard) = crate::telemetry::Observability::with_writer(
        crate::config::LogLevel::Debug,
        Output(output.clone()),
    )
    .unwrap();
    let (state, _directory) =
        test_state_with_activity(false, true, ServerActivity::with_observability(observer));
    let request = axum::http::Request::builder()
        .uri("/docs?private=secret-transcript")
        .header("authorization", "Bearer secret-token")
        .header("x-request-id", "secret-request-id")
        .header("x-fono-glossary", "secret-glossary")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        test_router(state).oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    guard.finish();
    let log = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    assert!(!log.contains("secret-"));
    let event: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(event["event"], "http_completed");
    assert_eq!(event["status"], 401);
}
fn test_router(state: Arc<ApiState>) -> Router {
    routes(Arc::clone(&state))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            observe_request,
        ))
        .layer(middleware::from_fn(crate::recognize::request_id_middleware))
        .with_state(RestApiState::new(
            "",
            SERVER_API_VERSION,
            state.activity.clone(),
        ))
}
async fn request(app: Router, method: &str, path: &str, token: Option<&str>) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-request-id", "metadata-test");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn discovery_and_docs_require_a_token_and_preserve_request_ids() {
    let (state, _directory) = test_state(false, false);
    let app = test_router(state);
    for path in ["/v1/capabilities", "/v1/models", "/openapi.json", "/docs"] {
        let response = request(app.clone(), "GET", path, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(response.headers()["x-request-id"], "metadata-test");
        assert_eq!(json(response).await["error"]["code"], "unauthorized");
    }
    let response = request(app.clone(), "GET", "/openapi.json", Some("recognize-test")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["info"]["version"], "2.0.0");
    let response = request(app, "GET", "/docs", Some("recognize-test")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let html = std::str::from_utf8(&body).unwrap();
    assert!(html.contains("Fono Server API v2"));
    assert!(!html.contains("<script src=") && !html.contains("cdn."));
}

#[tokio::test]
async fn capabilities_report_the_current_build_and_exact_feature_gates() {
    let (state, _directory) = test_state(false, true);
    let expected_bytes = state.config.server.max_upload_mb as u64 * 1024 * 1024;
    let response = request(
        test_router(state),
        "GET",
        "/v1/capabilities",
        Some("recognize-test"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = json(response).await;
    assert_eq!(value["api_version"], 2);
    assert_eq!(value["version"], env!("FONO_PRODUCT_VERSION"));
    assert_eq!(value["max_upload_bytes"], expected_bytes);
    assert_eq!(value["features"]["client_model_load"], false);
    assert_eq!(value["features"]["metrics"], true);
    assert_eq!(value["devices"].as_array().unwrap().len(), 1);
    assert_eq!(value["devices"][0]["kind"], "cpu");
    assert!(value["devices"][0]["threads"].as_u64().unwrap() > 0);
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(
        !encoded.contains("cuda")
            && !encoded.contains("/home/")
            && !encoded.contains("tokens_file")
    );
}

#[tokio::test]
async fn inventory_distinguishes_unloaded_from_invalid_without_exposing_paths() {
    let (state, directory) = test_state(false, false);
    let artifact = WhisperArtifact::all()
        .unwrap()
        .into_iter()
        .find(|artifact| artifact.variant == "tiny")
        .unwrap();
    let path = directory.0.join(&artifact.filename);
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(artifact.bytes).unwrap();
    let app = test_router(state);
    let response = request(app.clone(), "GET", "/v1/models", Some("recognize-test")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = json(response).await;
    assert_eq!(value["data"].as_array().unwrap().len(), 1);
    assert_eq!(value["data"][0]["id"], "whisper:tiny");
    assert_eq!(value["data"][0]["fono"]["state"], "unloaded");
    assert_eq!(value["data"][0]["fono"]["disk_state"], "present_unverified");
    assert_eq!(value["data"][0]["fono"]["features"]["glossary"], true);
    assert!(!serde_json::to_string(&value)
        .unwrap()
        .contains(&directory.0.to_string_lossy().to_string()));
    file.set_len(1).unwrap();
    let invalid = json(request(app, "GET", "/v1/models", Some("recognize-test")).await).await;
    assert_eq!(invalid["data"][0]["fono"]["state"], "failed");
    assert_eq!(invalid["data"][0]["fono"]["disk_state"], "invalid");
}

#[tokio::test]
async fn admin_commands_enforce_scope_gate_default_protection_and_verification() {
    let (disabled, _directory) = test_state(false, false);
    let response = request(
        test_router(disabled),
        "POST",
        "/v1/models/whisper:tiny/load",
        Some("admin-test"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json(response).await["error"]["code"],
        "model_control_disabled"
    );
    let (enabled, directory) = test_state(true, false);
    let default_id = enabled.config.models.default.clone();
    let app = test_router(enabled);
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/v1/models/whisper:tiny/load",
            Some("recognize-test")
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = request(
        app.clone(),
        "POST",
        &format!("/v1/models/{default_id}/unload"),
        Some("admin-test"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json(response).await["error"]["code"],
        "default_model_resident"
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/v1/models/unknown/load",
            Some("admin-test")
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            "/v1/models/whisper:tiny/load",
            Some("admin-test")
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let artifact = WhisperArtifact::all()
        .unwrap()
        .into_iter()
        .find(|artifact| artifact.variant == "tiny")
        .unwrap();
    std::fs::write(directory.0.join(artifact.filename), b"invalid model").unwrap();
    let response = request(
        app.clone(),
        "POST",
        "/v1/models/whisper:tiny/load",
        Some("admin-test"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(response).await["error"]["code"], "model_invalid");
    let response = request(
        app,
        "POST",
        "/v1/models/whisper:tiny/unload",
        Some("admin-test"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["state"], "unloaded");
}

#[tokio::test]
async fn metrics_require_admin_and_report_only_bounded_aggregates() {
    let (disabled, _directory) = test_state(false, false);
    assert_eq!(
        request(test_router(disabled), "GET", "/metrics", Some("admin-test"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let (state, directory) = test_state(false, true);
    let app = test_router(state);
    assert_eq!(
        request(app.clone(), "GET", "/metrics", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(app.clone(), "GET", "/metrics", Some("recognize-test"))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = request(app, "GET", "/metrics", Some("admin-test")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.contains("fono_http_requests_total 3\n"));
    assert!(body.contains("fono_http_errors_total 2\n"));
    assert!(body.contains("fono_http_request_duration_seconds_count 2\n"));
    assert!(body.contains("fono_jobs{state=\"queued\"} 2\n"));
    assert!(body.contains("fono_jobs{state=\"completed\"} 3\n"));
    for forbidden in [
        "admin-test",
        "recognize-test",
        "metadata-test",
        "/metrics",
        &directory.0.to_string_lossy(),
    ] {
        assert!(!body.contains(forbidden));
    }
}

#[tokio::test]
async fn cancelled_http_future_releases_the_inflight_gauge() {
    let (state, _directory) = test_state(false, true);
    let app: Router = Router::new()
        .route("/pending", get(std::future::pending::<&'static str>))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            observe_request,
        ));
    let pending = tokio::spawn(async move {
        app.oneshot(
            Request::builder()
                .uri("/pending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while state.requests.inflight.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    pending.abort();
    let _ = pending.await;
    assert_eq!(state.requests.inflight.load(Ordering::Relaxed), 0);
    let mut metrics = String::new();
    state.activity.observability.render(&mut metrics);
    assert!(metrics.contains("fono_http_request_duration_seconds_count 0\n"));
}

#[test]
fn openapi_references_resolve_and_cover_every_standalone_route() {
    let specification: serde_json::Value = serde_json::from_str(OPENAPI).unwrap();
    assert_eq!(specification["openapi"], "3.0.3");
    for path in [
        "/",
        "/panel/",
        "/healthz",
        "/v1/health",
        "/v1/capabilities",
        "/v1/models",
        "/v1/recognize",
        "/v1/audio/transcriptions",
        "/v1/transcriptions",
        "/v1/transcription-jobs",
        "/v1/transcription-jobs/{id}",
        "/v1/transcription-jobs/{id}/cancel",
        "/v1/models/{id}/load",
        "/v1/models/{id}/unload",
        "/metrics",
        "/openapi.json",
        "/docs",
    ] {
        assert!(specification["paths"].get(path).is_some(), "{path}");
    }
    fn check(value: &serde_json::Value, root: &serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if key == "$ref" {
                        let reference = value.as_str().unwrap().strip_prefix('#').unwrap();
                        assert!(root.pointer(reference).is_some(), "{reference}");
                    } else {
                        check(value, root);
                    }
                }
            }
            serde_json::Value::Array(array) => {
                for value in array {
                    check(value, root);
                }
            }
            _ => {}
        }
    }
    check(&specification, &specification);
    assert_eq!(
        specification["paths"]["/healthz"]["get"]["security"],
        serde_json::json!([])
    );
}
