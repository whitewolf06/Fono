use std::{
    fmt,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, OnceLock},
    time::Duration,
};

use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Client, RequestBuilder, Url,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::{runtime::Runtime, sync::Semaphore};
use zeroize::Zeroizing;

use super::{failure, RemoteError, RemoteErrorKind};
use crate::{error::AppResult, recognizer::check_cancellation, ActivityGuard};
use fono_core::OperationCancellation;

const CONCURRENT_REQUESTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteTimeouts {
    pub connect: Duration,
    pub response_base: Duration,
    pub audio_multiplier: f32,
}
impl Default for RemoteTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_millis(1500),
            response_base: Duration::from_secs(5),
            audio_multiplier: 1.0,
        }
    }
}

/// Only HTTPS, or loopback HTTP for a protected SSH tunnel. No downgrade flag,
/// credentials in URLs, redirects or ambient proxies. Not serializable.
#[derive(Clone)]
pub struct RemoteEndpoint {
    url: Url,
    authorization: HeaderValue,
    certificate: Option<reqwest::Certificate>,
    certificate_identity: Option<[u8; 32]>,
    timeouts: RemoteTimeouts,
}
impl fmt::Debug for RemoteEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteEndpoint").finish_non_exhaustive()
    }
}
impl RemoteEndpoint {
    pub fn new(url: &str, token: &str) -> AppResult<Self> {
        let invalid = || RemoteError::new(RemoteErrorKind::InvalidEndpoint);
        let url = Url::parse(url).map_err(|_| invalid())?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || token.is_empty()
            || token.len() > 4096
            || !token.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
            })
        {
            return Err(invalid().into());
        }
        let value = Zeroizing::new(format!("Bearer {token}"));
        let mut authorization = HeaderValue::from_str(&value).map_err(|_| invalid())?;
        authorization.set_sensitive(true);
        Ok(Self {
            url,
            authorization,
            certificate: None,
            certificate_identity: None,
            timeouts: RemoteTimeouts::default(),
        })
    }

    /// Trust an explicitly supplied private CA; certificate/hostname validation
    /// remains enabled. The caller owns reading the PEM from secure storage.
    pub fn with_ca_pem(mut self, pem: &[u8]) -> AppResult<Self> {
        if pem.len() > 256 * 1024 {
            return failure(RemoteErrorKind::InvalidEndpoint);
        }
        self.certificate = Some(
            reqwest::Certificate::from_pem(pem)
                .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidEndpoint))?,
        );
        self.certificate_identity = Some(Sha256::digest(pem).into());
        Ok(self)
    }

    pub fn with_timeouts(mut self, timeouts: RemoteTimeouts) -> AppResult<Self> {
        if timeouts.connect.is_zero()
            || timeouts.connect > Duration::from_secs(30)
            || timeouts.response_base.is_zero()
            || timeouts.response_base > Duration::from_secs(300)
            || !timeouts.audio_multiplier.is_finite()
            || !(0.0..=10.0).contains(&timeouts.audio_multiplier)
        {
            return failure(RemoteErrorKind::InvalidEndpoint);
        }
        self.timeouts = timeouts;
        Ok(self)
    }

    fn same_configuration(&self, other: &Self) -> bool {
        self.url == other.url
            && self.authorization == other.authorization
            && self.certificate_identity == other.certificate_identity
            && self.timeouts == other.timeouts
    }
}

pub(super) struct Transport {
    client: Client,
    endpoint: RemoteEndpoint,
    activity: Arc<dyn ActivityGuard>,
    slots: Arc<Semaphore>,
}

impl Transport {
    pub(super) fn new(
        endpoint: RemoteEndpoint,
        activity: Arc<dyn ActivityGuard>,
    ) -> AppResult<Self> {
        let mut builder = Client::builder()
            .use_rustls_tls()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .retry(reqwest::retry::never())
            .connect_timeout(endpoint.timeouts.connect)
            .pool_max_idle_per_host(CONCURRENT_REQUESTS)
            .pool_idle_timeout(Duration::from_secs(30))
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        if endpoint.url.host_str() == Some("localhost") {
            // A plaintext loopback exception must never follow external DNS.
            builder = builder.resolve(
                "localhost",
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            );
        }
        if let Some(cert) = &endpoint.certificate {
            builder = builder.add_root_certificate(cert.clone());
        }
        let client = builder
            .build()
            .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidEndpoint))?;
        Ok(Self {
            client,
            endpoint,
            activity,
            slots: Arc::new(Semaphore::new(CONCURRENT_REQUESTS)),
        })
    }

    pub(super) fn is_idle(&self) -> bool {
        self.slots.available_permits() == CONCURRENT_REQUESTS
    }
    pub(super) fn same_configuration(&self, endpoint: &RemoteEndpoint) -> bool {
        self.endpoint.same_configuration(endpoint)
    }
    pub(super) fn deadline(&self, samples: usize) -> Duration {
        self.endpoint.timeouts.response_base
            + Duration::from_secs_f64(
                (samples as f64 / 16_000.0 * self.endpoint.timeouts.audio_multiplier as f64)
                    .min(36_000.0),
            )
    }
    pub(super) fn discovery_deadline(&self) -> Duration {
        self.endpoint.timeouts.response_base
    }
    pub(super) fn get(&self, path: &str) -> RequestBuilder {
        self.client
            .get(
                self.endpoint
                    .url
                    .join(path)
                    .expect("constant relative route"),
            )
            .header(AUTHORIZATION, self.endpoint.authorization.clone())
    }
    pub(super) fn post(&self, path: &str) -> RequestBuilder {
        self.client
            .post(
                self.endpoint
                    .url
                    .join(path)
                    .expect("constant relative route"),
            )
            .header(AUTHORIZATION, self.endpoint.authorization.clone())
    }

    /// Only this task owns admission and the bounded request permit. The
    /// caller waits for actual future destruction; no early release on cancel.
    pub(super) fn call<T, F, Fut>(
        &self,
        cancel: &OperationCancellation,
        deadline: Duration,
        prepare: F,
    ) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce() -> AppResult<Fut>,
        Fut: Future<Output = AppResult<T>> + Send + 'static,
    {
        check_cancellation(cancel)?;
        let lease = self.activity.lease()?;
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| RemoteError::new(RemoteErrorKind::Busy))?;
        let work = prepare()?;
        let cancellation = cancel.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        runtime()?.spawn(async move {
            let outcome = controlled(work, &cancellation, deadline).await;
            drop(permit);
            drop(lease);
            let _ = send.send(outcome);
        });
        let outcome = receive
            .recv()
            .map_err(|_| RemoteError::new(RemoteErrorKind::Internal))?;
        check_cancellation(cancel)?;
        outcome
    }

    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        limit: usize,
    ) -> AppResult<T> {
        self.json_response(request, limit, false)
            .await
            .map(|(_, body)| body)
    }

    /// Health uses a valid typed JSON document for both ready (200) and
    /// not-ready (503); all other routes retain their existing error boundary.
    pub(super) async fn health_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        limit: usize,
    ) -> AppResult<(u16, T)> {
        self.json_response(request, limit, true).await
    }

    async fn json_response<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        limit: usize,
        health: bool,
    ) -> AppResult<(u16, T)> {
        let mut response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(network)?;
        let status = response.status();
        let health_unready = health && status.as_u16() == 503;
        if !status.is_success() && !health_unready {
            // Do not read/log an untrusted error body (which can echo secrets).
            let kind = match status.as_u16() {
                401 | 403 => RemoteErrorKind::Unauthorized,
                429 => RemoteErrorKind::Busy,
                404 => RemoteErrorKind::ModelNotFound,
                408 | 504 => RemoteErrorKind::Timeout,
                503 => RemoteErrorKind::ModelNotReady,
                502 => RemoteErrorKind::WorkerFailed,
                413 | 415 => RemoteErrorKind::UnsupportedAudio,
                400 | 422 => RemoteErrorKind::InvalidRequest,
                _ => RemoteErrorKind::InvalidResponse,
            };
            return Err(RemoteError {
                kind,
                status: Some(status.as_u16()),
                retry_after_seconds: response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|s| s.to_str().ok())
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(|s| s.min(3600)),
            }
            .into());
        }
        if (status.as_u16() != 200 && !health_unready)
            || response.content_length().is_some_and(|n| n > limit as u64)
            || response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| {
                    v.split(';')
                        .next()
                        .is_none_or(|t| !t.trim().eq_ignore_ascii_case("application/json"))
                })
        {
            return failure(RemoteErrorKind::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return failure(RemoteErrorKind::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(|body| (status.as_u16(), body))
            .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidResponse).into())
    }
}

fn network(error: reqwest::Error) -> crate::error::AppError {
    RemoteError::new(if error.is_timeout() {
        RemoteErrorKind::Timeout
    } else {
        RemoteErrorKind::Network
    })
    .into()
}

async fn controlled<T>(
    work: impl Future<Output = AppResult<T>>,
    cancellation: &OperationCancellation,
    deadline: Duration,
) -> AppResult<T> {
    tokio::select! {
        biased;
        _ = async { loop { if cancellation.is_cancelled() { break; } tokio::time::sleep(Duration::from_millis(10)).await; } } => {
            check_cancellation(cancellation)?;
            unreachable!()
        },
        _ = tokio::time::sleep(deadline) => failure(RemoteErrorKind::Timeout),
        result = work => result,
    }
}

fn runtime() -> AppResult<&'static Runtime> {
    // One process runtime, not one thread/runtime per request or endpoint. It
    // never drops on a Tauri/Tokio async thread during cancellation/teardown.
    static RUNTIME: OnceLock<Result<Runtime, ()>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("fono-remote")
                .enable_all()
                .build()
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| RemoteError::new(RemoteErrorKind::Internal).into())
}
