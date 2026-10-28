use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use super::*;
use crate::transcription_jobs::{
    ClientInflightLimit, JobState, NoCaptureGate, NoInteractiveActivity, PendingLimit,
    TranscriptionJob, TranscriptionJobQueue,
};
use crate::transcription_service::{TranscriptionRuntime, TranscriptionService};
use axum::{body::Body, http::Request};
use fono_core::OperationCancellation;
use fono_stt::NoopActivity;
use fono_types::Transcript;
use tower::ServiceExt;

fn test_state() -> RestApiState {
    RestApiState::new("test-token", 1, Arc::new(NoopActivity))
}

struct CompletingRuntime {
    requested_language: Arc<Mutex<Option<String>>>,
    pcm_samples: Arc<Mutex<Vec<i16>>>,
}

impl TranscriptionRuntime for CompletingRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        Ok("base".into())
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        _cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        *self.requested_language.lock().unwrap() = Some(language.into());
        *self.pcm_samples.lock().unwrap() = pcm_samples.to_vec();
        Ok(Transcript {
            text: "готово".into(),
            detected_language: Some(language.into()),
            audio_secs: Some(0.1),
            transcribe_secs: Some(0.1),
            device: Some("CPU".into()),
        })
    }
}

struct StubJobs {
    job: Mutex<TranscriptionJob>,
    submit_error: Option<TranscriptionServiceError>,
}

impl StubJobs {
    fn ready() -> Self {
        Self {
            job: Mutex::new(TranscriptionJob {
                id: "tr_0000000000000001".into(),
                requested_language: "auto".into(),
                file_name: None,
                progress: None,
                state: JobState::Queued,
                created_at_ms: 0,
                started_at_ms: None,
                finished_at_ms: None,
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
                requested_language: "auto".into(),
                file_name: None,
                progress: None,
                state: JobState::Queued,
                created_at_ms: 0,
                started_at_ms: None,
                finished_at_ms: None,
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
        request: TranscriptionRequest,
    ) -> Result<TranscriptionJob, TranscriptionServiceError> {
        match &self.submit_error {
            Some(error) => Err(error.clone()),
            None => {
                let mut job = self.job.lock().unwrap();
                job.requested_language = request.language;
                Ok(job.clone())
            }
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
        job.state = JobState::Cancelled;
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

#[tokio::test]
async fn health_requires_the_bearer_token() {
    let app = router(test_state());
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

struct RejectAllTokens;

impl TokenVerifier for RejectAllTokens {
    fn verify(&self, _token: &str) -> bool {
        false
    }
}

#[tokio::test]
async fn host_token_verifier_controls_authorization() {
    let app = router(test_state().with_token_verifier(Arc::new(RejectAllTokens)));
    let response = app
        .oneshot(
            authorized_request("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

struct ScopedTokens {
    admin_revoked: AtomicBool,
}

impl TokenVerifier for ScopedTokens {
    fn verify(&self, token: &str) -> bool {
        self.scope(token).is_some()
    }

    fn scope(&self, token: &str) -> Option<TokenScope> {
        match token {
            "recognize-token" => Some(TokenScope::Recognize),
            "admin-token" if !self.admin_revoked.load(Ordering::Acquire) => Some(TokenScope::Admin),
            _ => None,
        }
    }
}

async fn test_admin_route(State(state): State<RestApiState>, headers: HeaderMap) -> StatusCode {
    match authorize_headers(&headers, state.verifier.as_ref(), TokenScope::Admin) {
        TokenAuthorization::Allowed => StatusCode::OK,
        TokenAuthorization::Unauthorized => StatusCode::UNAUTHORIZED,
        TokenAuthorization::Forbidden => StatusCode::FORBIDDEN,
    }
}

async fn request_status(app: &Router, uri: &str, headers: &[(&str, &str)]) -> StatusCode {
    let mut request = Request::builder().uri(uri);
    for &(name, value) in headers {
        request = request.header(name, value);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn header_only_scope_check_distinguishes_unauthorized_and_forbidden() {
    let tokens = Arc::new(ScopedTokens {
        admin_revoked: AtomicBool::new(false),
    });
    let state = test_state().with_token_verifier(tokens.clone());
    // API-08 will introduce the real admin endpoint. This probe route exists
    // only in the test router and is never merged into the product router.
    let admin = Router::new()
        .route("/__test/admin", get(test_admin_route))
        .with_state(state.clone());

    assert_eq!(
        request_status(&admin, "/__test/admin", &[]).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request_status(&admin, "/__test/admin?token=admin-token", &[]).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request_status(
            &admin,
            "/__test/admin",
            &[(header::AUTHORIZATION.as_str(), "Bearer wrong-token")]
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request_status(
            &admin,
            "/__test/admin",
            &[
                (header::AUTHORIZATION.as_str(), "Bearer admin-token"),
                (header::AUTHORIZATION.as_str(), "Bearer admin-token"),
            ]
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request_status(
            &admin,
            "/__test/admin",
            &[(header::AUTHORIZATION.as_str(), "Bearer recognize-token")]
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_status(
            &admin,
            "/__test/admin",
            &[(header::AUTHORIZATION.as_str(), "Bearer admin-token")]
        )
        .await,
        StatusCode::OK
    );

    let product = router(state);
    assert_eq!(
        request_status(
            &product,
            "/v1/health",
            &[(header::AUTHORIZATION.as_str(), "Bearer recognize-token")]
        )
        .await,
        StatusCode::OK
    );
    tokens.admin_revoked.store(true, Ordering::Release);
    assert_eq!(
        request_status(
            &admin,
            "/__test/admin",
            &[(header::AUTHORIZATION.as_str(), "Bearer admin-token")]
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn openapi_is_available_to_authorized_local_clients() {
    let app = router(test_state());
    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            authorized_request("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );
}

#[tokio::test]
async fn browser_documentation_is_public_but_does_not_embed_the_token() {
    let app = router(test_state());
    let response = app
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("test-token"));
}

#[tokio::test]
async fn jobs_support_submit_lookup_and_cancellation() {
    let jobs: Arc<dyn TranscriptionJobs> = Arc::new(StubJobs::ready());
    let app = router(test_state().with_jobs(jobs));
    let request = TranscriptionRequest {
        pcm_samples: vec![1, -1],
        language: "ru".into(),
        model: None,
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
    let submitted_body = axum::body::to_bytes(submitted.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&submitted_body).unwrap()["requested_language"],
        "ru"
    );

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
    let app = router(test_state().with_jobs(jobs));
    let request = TranscriptionRequest {
        pcm_samples: vec![1],
        language: "auto".into(),
        model: Some("base".into()),
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
    assert!(response.headers().get(header::RETRY_AFTER).is_none());
}

struct NamedTokens;

impl TokenVerifier for NamedTokens {
    fn verify(&self, token: &str) -> bool {
        matches!(token, "client-a" | "client-b")
    }

    fn principal(&self, token: &str) -> Option<TokenPrincipal> {
        self.verify(token).then(|| TokenPrincipal {
            client_key: token.to_owned(),
            scope: TokenScope::Recognize,
        })
    }
}

#[tokio::test]
async fn v2_jobs_limit_each_token_and_retry_after_busy() {
    let queue = Arc::new(
        TranscriptionJobQueue::new(
            TranscriptionService::new(CompletingRuntime {
                requested_language: Arc::new(Mutex::new(None)),
                pcm_samples: Arc::new(Mutex::new(Vec::new())),
            }),
            NoInteractiveActivity,
            2,
            Arc::new(NoopActivity),
            Arc::new(NoCaptureGate),
        )
        .with_client_limit(ClientInflightLimit::new(1)),
    );
    let mut state = test_state().with_jobs(queue.clone());
    state.protocol_version = 2;
    let app = router(state.with_token_verifier(Arc::new(NamedTokens)));
    let body = serde_json::to_vec(&TranscriptionRequest {
        pcm_samples: vec![1],
        language: "auto".into(),
        model: None,
    })
    .unwrap();
    let submit = |token: &'static str| {
        Request::builder()
            .uri("/v1/transcription-jobs")
            .method("POST")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.clone()))
            .unwrap()
    };

    assert_eq!(
        app.clone()
            .oneshot(submit("client-a"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let busy = app.clone().oneshot(submit("client-a")).await.unwrap();
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy.headers().get(header::RETRY_AFTER).unwrap(), "1");
    let busy_body = axum::body::to_bytes(busy.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: serde_json::Value = serde_json::from_slice(&busy_body).unwrap();
    assert_eq!(error["error"]["code"], "busy");
    assert_eq!(error["error"]["retryable"], true);
    assert!(error["error"]["request_id"].as_str().is_some());
    assert_eq!(
        app.oneshot(submit("client-b")).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
}

#[tokio::test]
async fn multipart_upload_reserves_client_and_queue_before_decoding() {
    let queue = Arc::new(
        TranscriptionJobQueue::new(
            TranscriptionService::new(CompletingRuntime {
                requested_language: Arc::new(Mutex::new(None)),
                pcm_samples: Arc::new(Mutex::new(Vec::new())),
            }),
            NoInteractiveActivity,
            1,
            Arc::new(NoopActivity),
            Arc::new(NoCaptureGate),
        )
        .with_client_limit(ClientInflightLimit::new(1))
        .with_pending_limit(PendingLimit::new(1)),
    );
    let first = queue
        .submit_for_client(
            "client-a",
            TranscriptionRequest {
                pcm_samples: vec![1],
                language: "auto".into(),
                model: None,
            },
        )
        .unwrap();
    let mut state = test_state().with_jobs(queue.clone());
    state.protocol_version = 2;
    let app = router(state.with_token_verifier(Arc::new(NamedTokens)));
    let malformed_upload = || {
        Request::builder()
            .uri("/v1/transcriptions")
            .method("POST")
            .header(header::AUTHORIZATION, "Bearer client-a")
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=bad")
            .body(Body::from("invalid multipart"))
            .unwrap()
    };
    let busy = app.clone().oneshot(malformed_upload()).await.unwrap();
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy.headers().get(header::RETRY_AFTER).unwrap(), "1");
    let malformed_json = || {
        Request::builder()
            .uri("/v1/transcription-jobs")
            .method("POST")
            .header(header::AUTHORIZATION, "Bearer client-a")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{invalid"))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(malformed_json())
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS,
    );
    queue.cancel(&first.id).unwrap();
    let invalid = app.clone().oneshot(malformed_upload()).await.unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        app.oneshot(malformed_json()).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn multipart_wav_without_model_uses_the_selected_model_and_cleans_up_upload() {
    let upload_dir = std::env::temp_dir().join(format!("fono-rest-test-{}", uuid::Uuid::new_v4()));
    let requested_language = Arc::new(Mutex::new(None));
    let pcm_samples = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(TranscriptionJobQueue::new(
        TranscriptionService::new(CompletingRuntime {
            requested_language: Arc::clone(&requested_language),
            pcm_samples: Arc::clone(&pcm_samples),
        }),
        NoInteractiveActivity,
        1,
        Arc::new(NoopActivity),
        Arc::new(NoCaptureGate),
    ));
    let jobs: Arc<dyn TranscriptionJobs> = queue.clone();
    let app = router(
        test_state()
            .with_jobs(jobs)
            .with_upload_dir(upload_dir.clone()),
    );
    let boundary = "fono-test-boundary";
    let mut body = Vec::new();
    multipart_text(&mut body, boundary, "language", "ru");
    body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"sample.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
            )
            .as_bytes(),
        );
    body.extend_from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/audio/tone-16k-mono.wav"
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
    let submitted: TranscriptionJob = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(submitted.requested_language, "ru");

    let completed = queue.run_next().expect("queued upload completes");
    assert_eq!(completed.result.expect("completed result").model, "base");
    assert_eq!(requested_language.lock().unwrap().as_deref(), Some("ru"));
    let decoded_samples = pcm_samples.lock().unwrap();
    assert_eq!(decoded_samples.len(), 1600);
    assert!(decoded_samples.iter().any(|sample| *sample > 0));
    assert!(decoded_samples.iter().any(|sample| *sample < 0));
    assert!(std::fs::read_dir(&upload_dir).unwrap().next().is_none());
    std::fs::remove_dir_all(upload_dir).unwrap();
}

#[tokio::test]
async fn upload_respects_host_file_limit_before_decoding() {
    let upload_dir = std::env::temp_dir().join(format!("fono-rest-test-{}", uuid::Uuid::new_v4()));
    let app = router(
        test_state()
            .with_upload_dir(upload_dir.clone())
            .with_upload_policy(crate::audio_ingest::AudioIngestPolicy {
                max_file_bytes: 32,
                max_duration_seconds: 1,
            }),
    );
    let boundary = "fono-limited-upload";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"sample.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/audio/tone-16k-mono.wav"
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
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
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
async fn serves_on_a_caller_bound_listener_and_stops_cleanly() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_with_shutdown(
        router(test_state()),
        listener,
        async move {
            let _ = stopped.await;
        },
    ));

    let response = reqwest::Client::new()
        .get(format!("http://{address}/v1/health"))
        .bearer_auth("test-token")
        .send()
        .await
        .expect("call health endpoint");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}
