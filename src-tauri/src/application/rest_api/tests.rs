use std::sync::{Arc, Mutex};

use super::*;
use crate::application::transcription_jobs::{
    NoInteractiveActivity, TranscriptionJob, TranscriptionJobQueue,
};
use crate::application::transcription_service::{TranscriptionRuntime, TranscriptionService};
use crate::error::AppResult;
use crate::types::Transcript;
use axum::{body::Body, http::Request};

#[tokio::test]
async fn shutdown_still_signals_once_after_its_token_mutex_is_poisoned() {
    let (sender, receiver) = oneshot::channel();
    let server = Arc::new(RestApiServer {
        address: loopback_addr(0),
        shutdown: Mutex::new(Some(sender)),
    });
    let poisoned_server = Arc::clone(&server);
    assert!(std::thread::spawn(move || {
        let _guard = poisoned_server.shutdown.lock().unwrap();
        panic!("poison only the shutdown token");
    })
    .join()
    .is_err());
    server.shutdown();
    server.shutdown();
    receiver.await.unwrap();
}
use fono_core::OperationCancellation;
use tower::ServiceExt;

struct CompletingRuntime {
    requested_language: Arc<Mutex<Option<String>>>,
    pcm_samples: Arc<Mutex<Vec<i16>>>,
}

impl TranscriptionRuntime for CompletingRuntime {
    fn configured_model(&self) -> AppResult<String> {
        Ok("base".into())
    }

    fn transcribe(
        &self,
        pcm_samples: &[i16],
        language: &str,
        _cancellation: OperationCancellation,
    ) -> AppResult<Transcript> {
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
                state: crate::application::transcription_jobs::JobState::Queued,
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
                state: crate::application::transcription_jobs::JobState::Queued,
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
async fn openapi_is_available_to_authorized_local_clients() {
    let app = router(RestApiState::new("test-token", 1));
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
    let app = router(RestApiState::new("test-token", 1));
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
    let app = router(RestApiState::new("test-token", 1).with_jobs(jobs));
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
    let app = router(RestApiState::new("test-token", 1).with_jobs(jobs));
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
    ));
    let jobs: Arc<dyn TranscriptionJobs> = queue.clone();
    let app = router(
        RestApiState::new("test-token", 1)
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
