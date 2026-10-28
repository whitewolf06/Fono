use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use super::*;

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

#[test]
fn always_binds_to_ipv4_loopback() {
    assert_eq!(loopback_addr(0).ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
}

#[tokio::test]
async fn started_server_is_loopback_only_and_stops_cleanly() {
    let server = start(
        RestApiState::new("test-token", 1, Arc::new(fono_stt::NoopActivity)),
        0,
    )
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
