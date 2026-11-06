use super::*;

fn batch_fixture() -> Fixture {
    let fixture = Fixture::new();
    {
        let mut metadata = fixture.metadata.lock().unwrap();
        metadata.0["audio_formats"] = json!([PCM_TYPE, "audio/wav"]);
        metadata.0["features"]["jobs"] = json!(true);
    }
    fixture
}

fn connect(fixture: &Fixture) -> AppResult<RemoteRecognizer> {
    RemoteRecognizer::connect(
        fixture.endpoint(),
        model(),
        Arc::new(crate::NoopActivity),
        &OperationCancellation::default(),
    )
}

fn connect_batch(fixture: &Fixture) -> AppResult<RemoteTranscriptionClient> {
    RemoteTranscriptionClient::connect(
        fixture.endpoint(),
        model(),
        Arc::new(crate::NoopActivity),
        &OperationCancellation::default(),
    )
}

fn assert_no_audio(fixture: &Fixture) {
    let requests = fixture.requests.lock().unwrap();
    assert!(!requests
        .iter()
        .any(|request| request.headers.starts_with("POST ")));
    assert!(requests.iter().all(|request| request.body.is_empty()));
}

#[test]
fn minimal_old_protocol_is_classified_before_deserializing_v2_fields_everywhere() {
    for version in [0, 1] {
        let fixture = batch_fixture();
        fixture.metadata.lock().unwrap().0 = json!({
            "api_version":version,
            "product":"fono-server",
            // Irrelevant old-version fields must not be parsed as v2 first.
            "features":"PRIVATE_old_schema"
        });
        assert_eq!(
            kind(connect(&fixture).unwrap_err()).kind,
            RemoteErrorKind::Incompatible
        );
        assert_eq!(
            kind(connect_batch(&fixture).unwrap_err()).kind,
            RemoteErrorKind::Incompatible
        );
        assert_eq!(
            kind(
                inspect_server(
                    fixture.endpoint(),
                    Some(&model()),
                    Arc::new(crate::NoopActivity),
                    &OperationCancellation::default()
                )
                .unwrap_err()
            )
            .kind,
            RemoteErrorKind::Incompatible
        );
        assert_eq!(
            kind(
                probe_health(
                    fixture.endpoint(),
                    &model(),
                    Arc::new(crate::NoopActivity),
                    &OperationCancellation::default()
                )
                .unwrap_err()
            )
            .kind,
            RemoteErrorKind::Incompatible
        );
        assert!(fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.path == "/v1/capabilities"));
        assert_no_audio(&fixture);
    }
}

#[test]
fn absent_capabilities_route_is_a_protocol_incompatibility() {
    let fixture = batch_fixture();
    *fixture.mode.lock().unwrap() = Mode::Status(404);
    for error in [
        connect(&fixture).unwrap_err(),
        connect_batch(&fixture).unwrap_err(),
        inspect_server(
            fixture.endpoint(),
            None,
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap_err(),
        probe_health(
            fixture.endpoint(),
            &model(),
            Arc::new(crate::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap_err(),
    ] {
        assert_eq!(kind(error).kind, RemoteErrorKind::Incompatible);
    }
    assert_no_audio(&fixture);
}

#[test]
fn another_product_is_incompatible_even_without_fono_version_fields() {
    let fixture = Fixture::new();
    fixture.metadata.lock().unwrap().0 = json!({"product":"different-server"});
    assert_eq!(
        kind(connect(&fixture).unwrap_err()).kind,
        RemoteErrorKind::Incompatible
    );
    assert_no_audio(&fixture);
}

#[test]
fn v2_missing_required_capabilities_is_distinct_from_an_old_protocol() {
    let changes: [fn(&mut Value); 4] = [
        |server| {
            server.as_object_mut().unwrap().remove("features");
        },
        |server| server["features"]["cancel_on_disconnect"] = json!(false),
        |server| server["audio_formats"] = json!(["audio/wav"]),
        |server| server["features"]["decoding"] = json!(["accurate"]),
    ];
    for change in changes {
        let fixture = Fixture::new();
        change(&mut fixture.metadata.lock().unwrap().0);
        assert_eq!(
            kind(connect(&fixture).unwrap_err()).kind,
            RemoteErrorKind::UnsupportedCapabilities
        );
        assert_eq!(
            kind(
                inspect_server(
                    fixture.endpoint(),
                    None,
                    Arc::new(crate::NoopActivity),
                    &OperationCancellation::default()
                )
                .unwrap_err()
            )
            .kind,
            RemoteErrorKind::UnsupportedCapabilities
        );
        assert_eq!(
            kind(
                probe_health(
                    fixture.endpoint(),
                    &model(),
                    Arc::new(crate::NoopActivity),
                    &OperationCancellation::default()
                )
                .unwrap_err()
            )
            .kind,
            RemoteErrorKind::UnsupportedCapabilities
        );
        assert_no_audio(&fixture);
    }
}

#[test]
fn selected_model_features_are_checked_by_connection_and_health_inspection() {
    let fixture = Fixture::new();
    fixture.metadata.lock().unwrap().1["data"][0]["fono"]
        .as_object_mut()
        .unwrap()
        .remove("features");
    assert_eq!(
        kind(connect(&fixture).unwrap_err()).kind,
        RemoteErrorKind::UnsupportedCapabilities
    );
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                Some(&model()),
                Arc::new(crate::NoopActivity),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::UnsupportedCapabilities
    );
    assert_eq!(
        kind(
            probe_health(
                fixture.endpoint(),
                &model(),
                Arc::new(crate::NoopActivity),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::UnsupportedCapabilities
    );
    assert_no_audio(&fixture);
}

#[test]
fn selected_model_without_languages_is_not_reported_as_a_compatible_connection() {
    let fixture = Fixture::new();
    fixture.metadata.lock().unwrap().1["data"][0]["fono"]["languages"] = json!([]);
    assert_eq!(
        kind(connect(&fixture).unwrap_err()).kind,
        RemoteErrorKind::UnsupportedCapabilities
    );
    assert_eq!(
        kind(
            inspect_server(
                fixture.endpoint(),
                Some(&model()),
                Arc::new(crate::NoopActivity),
                &OperationCancellation::default()
            )
            .unwrap_err()
        )
        .kind,
        RemoteErrorKind::UnsupportedCapabilities
    );
    assert_no_audio(&fixture);
}

#[test]
fn absent_optional_features_and_additive_protocol_metadata_keep_basic_recognition() {
    let fixture = Fixture::new();
    {
        let mut metadata = fixture.metadata.lock().unwrap();
        metadata.0["api_version"] = json!(MIN_SERVER_API_VERSION + 1);
        metadata.0["version"] = json!("99.0.0-new-build");
        metadata.0["worker_protocol"] = json!(999);
        metadata.0["features"] = json!({"cancel_on_disconnect":true,"decoding":["fast","future-mode"],"future_feature":{"value":"ignored"}});
        metadata.1["data"][0]["fono"]["features"] =
            json!({"decoding":["fast","future-mode"],"future_feature":true});
    }
    let client = connect(&fixture).unwrap();
    assert!(!client.capabilities().hotwords);
    assert!(!client.capabilities().window_transcription);
    assert!(!client.capabilities().decoding_options);
    client
        .transcribe(&[0; 1600], options(), &OperationCancellation::default())
        .unwrap();
    let requests = fixture.requests.lock().unwrap();
    let request = requests
        .iter()
        .find(|request| request.path.starts_with("/v1/recognize"))
        .unwrap();
    assert!(!request.path.contains("timestamps="));
    assert!(!request
        .headers
        .to_ascii_lowercase()
        .contains("x-fono-glossary"));
    assert!(!request.path.contains("future"));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/v1/capabilities")
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/v1/models")
            .count(),
        2
    );
}

#[test]
fn cached_recognizer_rejects_a_server_downgrade_before_audio() {
    let fixture = Fixture::new();
    let client = connect(&fixture).unwrap();
    fixture.metadata.lock().unwrap().0 = json!({"api_version":1,"product":"fono-server"});
    assert_eq!(
        kind(
            client
                .transcribe(&[0; 1600], options(), &OperationCancellation::default())
                .unwrap_err()
        )
        .kind,
        RemoteErrorKind::Incompatible
    );
    assert_no_audio(&fixture);
}

#[test]
fn cached_recognizer_rechecks_requested_global_and_model_features_before_audio() {
    for global in [false, true] {
        for feature in ["decoding", "glossary", "timestamps"] {
            let fixture = Fixture::new();
            let client = connect(&fixture).unwrap();
            {
                let mut metadata = fixture.metadata.lock().unwrap();
                let features = if global {
                    &mut metadata.0["features"]
                } else {
                    &mut metadata.1["data"][0]["fono"]["features"]
                };
                features[feature] = match feature {
                    "decoding" => json!(["fast"]),
                    "glossary" => json!(false),
                    _ => json!(["none"]),
                };
            }
            let cancellation = OperationCancellation::default();
            let error = match feature {
                "decoding" => client
                    .transcribe(
                        &[0; 1600],
                        RecognitionOptions {
                            decoding: fono_stt_protocol::DecodingOptions {
                                strategy: DecodingStrategy::Accurate,
                                ..Default::default()
                            },
                            ..options()
                        },
                        &cancellation,
                    )
                    .unwrap_err(),
                "glossary" => client
                    .transcribe(
                        &[0; 1600],
                        RecognitionOptions {
                            glossary: &["PRIVATE_glossary".into()],
                            ..options()
                        },
                        &cancellation,
                    )
                    .unwrap_err(),
                _ => client
                    .transcribe_window(
                        &[0; 1600],
                        WindowOptions {
                            recognition: options(),
                            context: None,
                            operation_id: 1,
                            audio_start_sample: 0,
                        },
                        &cancellation,
                    )
                    .unwrap_err(),
            };
            assert_eq!(kind(error).kind, RemoteErrorKind::UnsupportedCapabilities);
            assert_no_audio(&fixture);
        }
    }
}

#[test]
fn cached_recognizer_rechecks_current_audio_limits_and_selected_languages() {
    for smaller_limit in [false, true] {
        let fixture = Fixture::new();
        let client = connect(&fixture).unwrap();
        {
            let mut metadata = fixture.metadata.lock().unwrap();
            if smaller_limit {
                metadata.0["max_upload_bytes"] = json!(100);
            } else {
                metadata.1["data"][0]["fono"]["languages"] = json!(["ru"]);
            }
        }
        assert_eq!(
            kind(
                client
                    .transcribe(&[0; 1600], options(), &OperationCancellation::default())
                    .unwrap_err()
            )
            .kind,
            RemoteErrorKind::InvalidRequest
        );
        assert_no_audio(&fixture);
    }
}

#[test]
fn batch_requires_its_own_features_instead_of_interactive_options() {
    let fixture = batch_fixture();
    {
        let mut metadata = fixture.metadata.lock().unwrap();
        metadata.0["audio_formats"] = json!(["audio/wav"]);
        metadata.0["features"] = json!({"jobs":true});
        metadata.1["data"][0]["fono"]["features"] = json!({});
    }
    let client = connect_batch(&fixture).unwrap();
    *fixture.mode.lock().unwrap() = Mode::Handler(Arc::new(|request| {
        assert_eq!(request.path, "/v1/transcriptions");
        (
            202,
            json!({"id":"compatible-batch", "state":"completed", "result":{
                "protocol_version":1,"text":"fixture","detected_language":"en","audio_seconds":0.1,
                "transcribe_seconds":null,"model":"whisper:tiny","backend":"CPU"
            }}),
            Duration::ZERO,
        )
    }));
    let result = client
        .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
        .unwrap();
    assert_eq!(result.backend.as_deref(), Some("remote:cpu"));
    assert_eq!(result.transcribe_seconds, None);
}

#[test]
fn cached_batch_client_rechecks_protocol_and_job_support_before_submission() {
    for downgrade in [false, true] {
        let fixture = batch_fixture();
        let client = connect_batch(&fixture).unwrap();
        if downgrade {
            fixture.metadata.lock().unwrap().0 = json!({"api_version":1,"product":"fono-server"});
        } else {
            fixture.metadata.lock().unwrap().0["features"]["jobs"] = json!(false);
        }
        let error = client
            .transcribe_pcm_job(&[0; 1600], "en", &OperationCancellation::default(), &|_| {})
            .unwrap_err();
        assert_eq!(
            kind(error).kind,
            if downgrade {
                RemoteErrorKind::Incompatible
            } else {
                RemoteErrorKind::UnsupportedCapabilities
            }
        );
        assert_no_audio(&fixture);
    }
}

#[test]
fn malformed_or_unversioned_capability_document_never_becomes_compatible() {
    for document in [
        json!({"product":"fono-server"}),
        json!({"api_version":"PRIVATE", "product":"fono-server"}),
        json!({"api_version":2,"product":"fono-server","features":[]}),
    ] {
        let fixture = Fixture::new();
        fixture.metadata.lock().unwrap().0 = document;
        let error = connect(&fixture).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        assert_eq!(kind(error).kind, RemoteErrorKind::InvalidResponse);
        assert_no_audio(&fixture);
    }
}
