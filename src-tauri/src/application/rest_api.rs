//! Desktop composition for the shared transcription HTTP router.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tokio::sync::oneshot;

pub use fono_service::rest_api::{router, RestApiState};

/// Running loopback server. The shutdown token stops accepting local requests
/// before the desktop STT runtime shuts down.
pub struct RestApiServer {
    address: SocketAddr,
    shutdown: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl RestApiServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub fn shutdown(&self) {
        let sender = {
            let mut shutdown = match self.shutdown.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            shutdown.take()
        };
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

impl Drop for RestApiServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Bind desktop API to IPv4 loopback, regardless of the reusable router's
/// listener policy. Port zero is available for local tests.
pub async fn start(state: RestApiState, port: u16) -> std::io::Result<RestApiServer> {
    let listener = tokio::net::TcpListener::bind(loopback_addr(port)).await?;
    let address = listener.local_addr()?;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();

    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            fono_service::rest_api::serve_with_shutdown(router(state), listener, async move {
                let _ = shutdown_receiver.await;
            })
            .await
        {
            tracing::error!(%error, "local transcription REST server stopped unexpectedly");
        }
    });

    Ok(RestApiServer {
        address,
        shutdown: std::sync::Mutex::new(Some(shutdown_sender)),
    })
}

pub fn loopback_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[cfg(test)]
mod tests;
