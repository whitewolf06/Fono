use super::*;
use axum::{
    body::{to_bytes, Body},
    middleware,
    routing::post,
    Router,
};
use std::time::Duration;
use tower::ServiceExt;

fn limiter(rate: usize, burst: usize) -> RateLimiter {
    RateLimiter::new(RateLimitSettings {
        enabled: true,
        requests_per_minute: rate,
        burst,
    })
}

#[test]
fn quota_refills_exactly_and_isolated_credentials_do_not_share_it() {
    let limiter = limiter(60, 2);
    let now = Instant::now();
    assert_eq!(limiter.check_at("digest-a", now), Ok(()));
    assert_eq!(limiter.check_at("digest-a", now), Ok(()));
    assert_eq!(limiter.check_at("digest-a", now), Err(1));
    assert_eq!(limiter.check_at("digest-b", now), Ok(()));
    assert_eq!(
        limiter.check_at("digest-a", now + Duration::from_millis(999)),
        Err(1)
    );
    assert_eq!(
        limiter.check_at("digest-a", now + Duration::from_secs(1)),
        Ok(())
    );
    assert_eq!(self::limiter(1, 1).check_at("other", now), Ok(()));
}

#[test]
fn storage_is_bounded_and_full_buckets_are_the_only_evictable_entries() {
    let limiter = limiter(60, 1);
    let now = Instant::now();
    for i in 0..MAX_BUCKETS {
        limiter.check_at(&i.to_string(), now).unwrap();
    }
    assert_eq!(limiter.check_at("new-credential", now), Err(1));
    assert_eq!(limiter.buckets.lock().unwrap().len(), MAX_BUCKETS);
    assert_eq!(limiter.check_at("0", now), Err(1));
    assert_eq!(
        limiter.check_at("new-credential", now + Duration::from_secs(1)),
        Ok(())
    );
}

struct Tokens;
impl TokenVerifier for Tokens {
    fn verify(&self, value: &str) -> bool {
        value == "valid-test-token"
    }
}

#[tokio::test]
async fn limited_openai_upload_is_rejected_before_body_parsing_and_has_retry_after() {
    let state = Arc::new(RateState {
        limiter: limiter(1, 1),
        verifier: Arc::new(Tokens),
    });
    let app = Router::new()
        .route("/v1/audio/transcriptions", post(|| async { "accepted" }))
        .layer(middleware::from_fn_with_state(state.clone(), enforce))
        .layer(middleware::from_fn(crate::recognize::request_id_middleware));
    for expected in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let body = if expected == StatusCode::TOO_MANY_REQUESTS {
            Body::from_stream(futures_util::stream::poll_fn(
                |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
                    panic!("rate-limited request body must not be polled")
                },
            ))
        } else {
            Body::from("invalid-multipart-private-audio")
        };
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/v1/audio/transcriptions")
                    .header("authorization", "Bearer valid-test-token")
                    .header("x-request-id", "bounded-request")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(response.headers()[header::RETRY_AFTER], "60");
            assert_eq!(response.headers()["x-request-id"], "bounded-request");
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["error"]["code"], "rate_limited");
            assert_eq!(value["error"]["type"], "rate_limit_error");
            assert!(!String::from_utf8_lossy(&body).contains("private-audio"));
        }
    }
    assert_eq!(state.limiter.buckets.lock().unwrap().len(), 1);
}

#[test]
fn concurrent_requests_share_one_atomic_budget() {
    let limiter = Arc::new(limiter(1, 3));
    let now = Instant::now();
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let limiter = limiter.clone();
            std::thread::spawn(move || limiter.check_at("one-credential", now).is_ok())
        })
        .collect();
    let accepted = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 3);
}
