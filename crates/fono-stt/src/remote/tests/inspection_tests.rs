use super::*;

#[test]
fn inspection_reports_real_metadata_without_recognition_or_model_loading() {
    let fixture = Fixture::new();
    let snapshot = inspect_server(
        fixture.endpoint(),
        Some(&model()),
        Arc::new(TestActivity::default()),
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(snapshot.version, "0.8.30");
    assert_eq!(snapshot.api_version, 2);
    assert_eq!(snapshot.devices[1].name, "NVIDIA GeForce RTX 3060 Ti");
    assert_eq!(snapshot.models[0].id, model());
    assert!(snapshot.models[0].installed);
    assert_eq!(snapshot.state, RemoteInspectionState::Busy);
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        ["/v1/capabilities", "/v1/models", "/v1/health"]
    );
    assert!(requests.iter().all(|request| request.body.is_empty()));
    drop(requests);
    assert_eq!(fixture.recognition_count(), 0);
}

#[test]
fn inspection_classifies_authorization_api_and_model_errors_without_error_bodies() {
    let fixture = Fixture::new();
    *fixture.mode.lock().unwrap() = Mode::Status(401);
    let error = inspect_server(
        fixture.endpoint(),
        None,
        Arc::new(TestActivity::default()),
        &OperationCancellation::default(),
    )
    .unwrap_err();
    assert!(!error.to_string().contains("PRIVATE"));
    assert_eq!(kind(error).kind, RemoteErrorKind::Unauthorized);
    *fixture.mode.lock().unwrap() = Mode::Ok;
    fixture.metadata.lock().unwrap().0["api_version"] = json!(1);
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                None,
                Arc::new(TestActivity::default()),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Incompatible
    );
    fixture.metadata.lock().unwrap().0["api_version"] = json!(2);
    let missing = ModelId {
        variant: "absent".into(),
        ..model()
    };
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                Some(&missing),
                Arc::new(TestActivity::default()),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::ModelNotFound
    );
}

#[test]
fn inspection_accepts_typed_startup_health_and_rejects_unbounded_or_duplicate_metadata() {
    let fixture = Fixture::new();
    *fixture.mode.lock().unwrap() = Mode::HealthReply(503, health("starting", "loading"));
    let snapshot = inspect_server(
        fixture.endpoint(),
        None,
        Arc::new(TestActivity::default()),
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(snapshot.state, RemoteInspectionState::Starting);
    fixture.metadata.lock().unwrap().0["devices"][1]["name"] = json!("malformed\nPRIVATE");
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                None,
                Arc::new(TestActivity::default()),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
    fixture.metadata.lock().unwrap().0 = capabilities();
    let duplicate = fixture.metadata.lock().unwrap().1["data"][0].clone();
    fixture.metadata.lock().unwrap().1["data"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                None,
                Arc::new(TestActivity::default()),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::InvalidResponse
    );
}

#[test]
fn one_inspection_deadline_covers_a_hung_health_response_and_disconnects() {
    let fixture = Fixture::new();
    *fixture.mode.lock().unwrap() = Mode::Hang;
    let endpoint = fixture
        .endpoint()
        .with_timeouts(RemoteTimeouts {
            connect: Duration::from_millis(100),
            response_base: Duration::from_millis(100),
            audio_multiplier: 1.0,
        })
        .unwrap();
    assert_eq!(
        kind(
            inspect_server(
                endpoint,
                None,
                Arc::new(TestActivity::default()),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Timeout
    );
    wait(|| fixture.disconnected.load(Ordering::Acquire));
    assert_eq!(fixture.recognition_count(), 0);
}
