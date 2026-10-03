use super::*;
#[test]
fn a_forged_available_snapshot_cannot_supply_an_installer() {
    let service = UpdateService::default();
    service.transition(UpdatePhase::Available, "available");
    service.data.lock().snapshot.next_version = Some("99.0.0".into());
    assert!(require_candidate(&service.data.lock()).is_err());
}

#[tokio::test]
async fn dropped_install_await_keeps_exclusivity_and_finalizes_a_failure() {
    let service = Arc::new(UpdateService::default());
    service.transition(UpdatePhase::Installing, "installing");
    let serial = Arc::clone(&service.serial).try_lock_owned().unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let blocking_service = Arc::clone(&service);
    let handle = tokio::task::spawn_blocking(move || {
        let result = {
            let _serial = serial;
            started_tx.send(()).unwrap();
            finish_install(&blocking_service, || {
                release_rx.recv().unwrap();
                Err(())
            })
        };
        done_tx.send(result.is_err()).unwrap();
    });
    started_rx.recv().unwrap();
    drop(handle);
    assert!(service.serial.try_lock().is_err());
    assert_eq!(service.snapshot(false).phase, UpdatePhase::Installing);
    release_tx.send(()).unwrap();
    assert!(done_rx.await.unwrap());
    assert_eq!(service.snapshot(false).phase, UpdatePhase::Error);
    assert!(service.serial.try_lock().is_ok());
}

#[test]
fn installer_panic_after_shutdown_finalizes_an_actionable_error() {
    let service = UpdateService::default();
    service.transition(UpdatePhase::Installing, "installing");
    let result = finish_install(&service, || {
        service.shutdown_started.store(true, Ordering::Release);
        panic!("simulated installer panic");
    });
    assert!(result.is_err());
    assert_eq!(service.snapshot(false).phase, UpdatePhase::Error);
    assert!(service
        .snapshot(false)
        .message
        .contains("Перезапустите Fono"));
}

#[test]
fn insecure_tls_and_unbound_signed_versions_cannot_start_the_updater() {
    assert!(!secure_plugin_config(None));
    assert!(!secure_plugin_config(Some(&serde_json::json!({}))));
    assert!(secure_plugin_config(Some(
        &serde_json::json!({ "requireSignedVersion": true })
    )));
    for unsafe_field in [
        "dangerousInsecureTransportProtocol",
        "dangerousAcceptInvalidCerts",
        "dangerousAcceptInvalidHostnames",
    ] {
        assert!(!secure_plugin_config(Some(
            &serde_json::json!({ "requireSignedVersion": true, unsafe_field: true })
        )));
    }
}
