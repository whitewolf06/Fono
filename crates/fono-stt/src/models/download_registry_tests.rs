use super::*;

#[test]
fn registration_shares_cancellation_and_releases_only_its_identifier() {
    let id = format!("fono-model-registry-{}", uuid::Uuid::new_v4());
    let registration = DownloadRegistration::begin(id.clone(), ActivityLease::new(())).unwrap();
    assert!(matches!(
        DownloadRegistration::begin(id.clone(), ActivityLease::new(())),
        Err(AppError::Busy(_))
    ));
    assert!(cancel_download(&id));
    assert!(registration.cancellation.load(Ordering::Acquire));
    drop(registration);
    assert!(!cancel_download(&id));
    let next = DownloadRegistration::begin(id, ActivityLease::new(())).unwrap();
    assert!(!next.cancellation.load(Ordering::Acquire));
}

#[test]
fn retained_blocking_lease_outlives_the_async_registration_owner() {
    use std::sync::atomic::AtomicUsize;
    struct Token(Arc<AtomicUsize>);
    impl Drop for Token {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let released = Arc::new(AtomicUsize::new(0));
    let registration = DownloadRegistration::begin(
        format!("fono-model-retain-{}", uuid::Uuid::new_v4()),
        ActivityLease::new(Token(released.clone())),
    )
    .unwrap();
    let background = registration.blocking_lease();
    drop(registration);
    assert_eq!(released.load(Ordering::SeqCst), 0);
    drop(background);
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn progress_keeps_the_existing_phase_and_payload_fields() {
    let event = DownloadEvent {
        download_id: "whisper:tiny".into(),
        model: "ggml-tiny.bin".into(),
        phase: DownloadPhase::Verifying,
        downloaded_bytes: 42,
        total_bytes: Some(42),
    };
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "download_id":"whisper:tiny", "model":"ggml-tiny.bin", "phase":"verifying",
            "downloaded_bytes":42, "total_bytes":42
        })
    );
}
