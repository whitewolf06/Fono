//! Rustls transport and transactional certificate reload. Never logs PEM/errors.

use std::fs::{self, Metadata, OpenOptions};
use std::future::Future;
use std::io::{self, Read};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::Router;
#[cfg(unix)]
use fono_stt::ActivityGuard;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use zeroize::Zeroizing;

#[cfg(unix)]
use crate::config::LogLevel;
use crate::config::TlsSettings;
use crate::jobs::ServerActivity;
use crate::telemetry::Observability;
#[cfg(unix)]
use crate::telemetry::Phase;

const MAX_PEM_BYTES: u64 = 1_048_576;
const MAX_CONNECTIONS: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsError {
    File,
    Permissions,
    Certificate,
    PrivateKey,
    Validity,
    Mismatch,
    Unsupported,
    Task,
}
impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::File => "TLS file is unavailable, oversized or not regular",
            Self::Permissions => "TLS private key ownership or permissions are unsafe",
            Self::Certificate => "TLS certificate is invalid",
            Self::PrivateKey => "TLS private key is invalid, encrypted or unsupported",
            Self::Validity => "TLS certificate is expired or not yet valid",
            Self::Mismatch => "TLS certificate and private key do not match",
            Self::Unsupported => "TLS material validation requires the Ubuntu build",
            Self::Task => "TLS material validation task failed",
        })
    }
}
impl std::error::Error for TlsError {}

pub(crate) fn key_permissions(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.is_file()
            && metadata.mode() & 0o137 == 0
            && (metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn read_pem(path: &Path, private: bool) -> Result<Vec<u8>, TlsError> {
    // Reject FIFOs/devices before open, then check the actual open descriptor.
    if !fs::metadata(path).map_err(|_| TlsError::File)?.is_file() {
        return Err(TlsError::File);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| TlsError::File)?;
    let metadata = file.metadata().map_err(|_| TlsError::File)?;
    if !metadata.is_file() || metadata.len() > MAX_PEM_BYTES {
        return Err(TlsError::File);
    }
    if private && !key_permissions(&metadata) {
        return Err(TlsError::Permissions);
    }
    let mut bytes = Vec::new();
    file.take(MAX_PEM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| TlsError::File)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_PEM_BYTES {
        return Err(TlsError::File);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn validate_material(cert: &[u8], key: &[u8]) -> Result<(), TlsError> {
    use openssl::{asn1::Asn1Time, pkey::PKey, x509::X509};
    let cert = X509::from_pem(cert).map_err(|_| TlsError::Certificate)?;
    let key = PKey::private_key_from_pem_passphrase(key, b"").map_err(|_| TlsError::PrivateKey)?;
    let now = Asn1Time::days_from_now(0).map_err(|_| TlsError::Validity)?;
    if cert
        .not_before()
        .compare(&now)
        .map_err(|_| TlsError::Validity)?
        .is_gt()
        || !cert
            .not_after()
            .compare(&now)
            .map_err(|_| TlsError::Validity)?
            .is_gt()
    {
        return Err(TlsError::Validity);
    }
    if !cert
        .public_key()
        .map_err(|_| TlsError::Certificate)?
        .public_eq(&key)
    {
        return Err(TlsError::Mismatch);
    }
    Ok(())
}
#[cfg(not(unix))]
fn validate_material(_: &[u8], _: &[u8]) -> Result<(), TlsError> {
    Err(TlsError::Unsupported)
}

pub fn load_config(settings: &TlsSettings) -> Result<Arc<rustls::ServerConfig>, TlsError> {
    let cert = read_pem(&settings.cert, false)?;
    let key = Zeroizing::new(read_pem(&settings.key, true)?);
    validate_material(&cert, &key)?;
    let certificates = CertificateDer::pem_slice_iter(&cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TlsError::Certificate)?;
    if certificates.is_empty() {
        return Err(TlsError::Certificate);
    }
    let key = PrivateKeyDer::from_pem_slice(&key).map_err(|_| TlsError::PrivateKey)?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
    .map_err(|_| TlsError::Unsupported)?
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .map_err(|_| TlsError::PrivateKey)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

pub struct TlsState {
    #[cfg(unix)]
    settings: TlsSettings,
    current: RwLock<Arc<rustls::ServerConfig>>,
}
impl TlsState {
    pub fn new(settings: TlsSettings) -> Result<Self, TlsError> {
        let config = load_config(&settings)?;
        Ok(Self {
            #[cfg(unix)]
            settings,
            current: RwLock::new(config),
        })
    }
    fn current(&self) -> Arc<rustls::ServerConfig> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    #[cfg(unix)]
    async fn reload(&self, activity: Arc<ServerActivity>) -> Result<(), TlsError> {
        let settings = self.settings.clone();
        let lease = activity.cleanup_lease();
        let checked = tokio::task::spawn_blocking(move || {
            let _lease = lease;
            load_config(&settings)
        })
        .await
        .map_err(|_| TlsError::Task)??;
        // Publication belongs to this awaiting task, never to detached file I/O.
        *self.current.write().unwrap_or_else(|p| p.into_inner()) = checked;
        Ok(())
    }
}

#[cfg(unix)]
fn spawn_reloader(
    state: Arc<TlsState>,
    activity: Arc<ServerActivity>,
    observability: Arc<Observability>,
) -> io::Result<JoinHandle<()>> {
    let mut hup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    Ok(tokio::spawn(async move {
        while hup.recv().await.is_some() {
            let ok = state.reload(activity.clone()).await.is_ok();
            observability.event(
                if ok { LogLevel::Info } else { LogLevel::Warn },
                if ok {
                    Phase::TlsReloaded
                } else {
                    Phase::TlsReloadRejected
                },
            );
        }
    }))
}
#[cfg(not(unix))]
fn spawn_reloader(
    state: Arc<TlsState>,
    activity: Arc<ServerActivity>,
    observability: Arc<Observability>,
) -> io::Result<JoinHandle<()>> {
    let _ = (state, activity, observability);
    Ok(tokio::spawn(std::future::pending()))
}

pub async fn serve(
    router: Router,
    listener: tokio::net::TcpListener,
    state: Arc<TlsState>,
    activity: Arc<ServerActivity>,
    shutdown: impl Future<Output = ()> + Send,
) -> io::Result<()> {
    let reloader = spawn_reloader(
        state.clone(),
        activity.clone(),
        activity.observability.clone(),
    )?;
    let stop_reloader = AbortOnDrop(reloader);
    let (stop_tx, _) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    let result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            _ = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                let (stream, _) = match accepted { Ok(pair) => pair, Err(error) => break Err(error) };
                let config = state.current();
                let router = router.clone();
                let mut stop = stop_tx.subscribe();
                connections.spawn(async move {
                    if *stop.borrow() { return; }
                    let tls = tokio::select! {
                        biased;
                        _ = stop.changed() => return,
                        accepted = tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_rustls::TlsAcceptor::from(config).accept(stream)) => match accepted { Ok(Ok(tls)) => tls, _ => return },
                    };
                    let mut builder = Builder::new(TokioExecutor::new());
                    builder.http1().timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(10)).max_buf_size(32 * 1024);
                    builder.http2().timer(TokioTimer::new()).max_concurrent_streams(32).max_header_list_size(16 * 1024);
                    let connection = builder.serve_connection(TokioIo::new(tls), TowerToHyperService::new(router));
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = stop.changed() => { connection.as_mut().graceful_shutdown(); let _ = connection.await; }
                    }
                });
            }
        }
    };
    drop(stop_reloader);
    let _ = stop_tx.send(true);
    while connections.join_next().await.is_some() {}
    result
}
struct AbortOnDrop(JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(all(test, unix))]
mod tests;
