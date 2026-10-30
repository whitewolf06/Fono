//! Per-credential token buckets. Only verified credential digests reach storage.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use fono_service::rest_api::{authorize_principal_headers, TokenScope, TokenVerifier};

use crate::config::RateLimitSettings;
use crate::recognize::{HttpError, RequestId};

const CREDIT: u128 = 60_000_000_000;
const MAX_BUCKETS: usize = 4096;

struct Bucket {
    credit: u128,
    at: Instant,
}

pub struct RateLimiter {
    config: RateLimitSettings,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitSettings) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
        }
    }
    fn refill(&self, bucket: &Bucket, now: Instant) -> u128 {
        let added = now
            .saturating_duration_since(bucket.at)
            .as_nanos()
            .saturating_mul(self.config.requests_per_minute as u128);
        bucket
            .credit
            .saturating_add(added)
            .min(self.config.burst as u128 * CREDIT)
    }
    pub fn check(&self, credential: &str) -> Result<(), u64> {
        self.check_at(credential, Instant::now())
    }
    fn check_at(&self, credential: &str, now: Instant) -> Result<(), u64> {
        if !self.config.enabled {
            return Ok(());
        }
        let mut buckets = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        if !buckets.contains_key(credential) && buckets.len() >= MAX_BUCKETS {
            // Only fully recovered buckets may disappear: eviction cannot refill
            // an exhausted credential early. Rejected/unknown tokens add no keys.
            buckets
                .retain(|_, bucket| self.refill(bucket, now) < self.config.burst as u128 * CREDIT);
            if buckets.len() >= MAX_BUCKETS {
                return Err(1);
            }
        }
        let bucket = buckets.entry(credential.to_owned()).or_insert(Bucket {
            credit: self.config.burst as u128 * CREDIT,
            at: now,
        });
        bucket.credit = self.refill(bucket, now);
        bucket.at = now;
        if bucket.credit >= CREDIT {
            bucket.credit -= CREDIT;
            return Ok(());
        }
        let nanos = (CREDIT - bucket.credit).div_ceil(self.config.requests_per_minute as u128);
        Err(nanos
            .div_ceil(1_000_000_000)
            .max(1)
            .min(u128::from(u64::MAX)) as u64)
    }
}

pub struct RateState {
    pub limiter: RateLimiter,
    pub verifier: Arc<dyn TokenVerifier>,
}

/// Host checking and request-ID assignment wrap this layer; body extraction and
/// inference admission remain inside it. Scope checks still belong to routes.
pub async fn enforce(
    State(state): State<Arc<RateState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == "/healthz" || !state.limiter.config.enabled {
        return next.run(request).await;
    }
    let headers = request.headers().clone();
    let verifier = state.verifier.clone();
    let principal = tokio::task::spawn_blocking(move || {
        authorize_principal_headers(&headers, verifier.as_ref(), TokenScope::Recognize)
    })
    .await;
    let credential = match principal {
        Ok(Ok(principal)) => principal.client_key,
        // Route-specific authorization retains its existing wire format. These
        // callers cannot allocate buckets or consume a valid credential's quota.
        Ok(Err(_)) => return next.run(request).await,
        Err(_) => {
            let id = request
                .extensions()
                .get::<RequestId>()
                .cloned()
                .unwrap_or_else(|| RequestId(uuid::Uuid::new_v4().to_string()));
            return HttpError::internal().into_response(&id);
        }
    };
    match state.limiter.check(&credential) {
        Ok(()) => next.run(request).await,
        Err(seconds) => {
            let id = request
                .extensions()
                .get::<RequestId>()
                .cloned()
                .unwrap_or_else(|| RequestId(uuid::Uuid::new_v4().to_string()));
            let error = HttpError::rate_limited();
            let mut response = if request.uri().path() == "/v1/audio/transcriptions" {
                error.into_openai_response(&id)
            } else {
                error.into_response(&id)
            };
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).expect("integer retry delay"),
            );
            response
        }
    }
}

#[cfg(test)]
mod tests;
