use super::*;
use crate::tokens::{self, FileTokenVerifier};

fn client_state() -> (Arc<ApiState>, TestDirectory, String, String) {
    let (mut state, directory) = test_state(false, false);
    let path = directory.0.join("tokens");
    let admin = tokens::create(&path, "operator", TokenScope::Admin).unwrap();
    let client = tokens::create(&path, "desktop", TokenScope::Recognize).unwrap();
    let mutable = Arc::get_mut(&mut state).unwrap();
    mutable.config.server.tokens_file = path.clone();
    mutable.config.server.client_url = Some("http://fono:17832".into());
    mutable.verifier = Arc::new(FileTokenVerifier::open(path).unwrap());
    (state, directory, admin, client)
}

async fn call(app: Router, method: &str, path: &str, token: Option<&str>, body: Body) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-request-id", "client-test");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.oneshot(builder.body(body).unwrap()).await.unwrap()
}

#[tokio::test]
async fn clients_are_admin_only_and_authorization_precedes_body_polling() {
    let (state, _directory, _admin, client) = client_state();
    let app = test_router(state);
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(client.as_str()), StatusCode::FORBIDDEN),
    ] {
        for (method, path) in [("GET", "/v1/clients"), ("DELETE", "/v1/clients/desktop")] {
            let response = call(app.clone(), method, path, token, Body::empty()).await;
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["x-request-id"], "client-test");
        }
        let unread = Body::from_stream(futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<&'static str, std::io::Error>>> {
                panic!("unauthorized client creation body must not be polled")
            },
        ));
        let response = call(app.clone(), "POST", "/v1/clients", token, unread).await;
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn clients_create_list_reopen_and_revoke_without_secret_leaks_or_admin_changes() {
    let (state, directory, admin, _client) = client_state();
    let app = test_router(state);
    let created = call(
        app.clone(),
        "POST",
        "/v1/clients",
        Some(&admin),
        Body::from(r#"{"name":"test-app"}"#),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
    let created = json(created).await;
    assert_eq!(created["scope"], "recognize");
    assert_eq!(created["name"], "test-app");
    assert!(created["created_at"].as_str().unwrap().ends_with('Z'));
    let token = created["token"].as_str().unwrap();
    assert!(token.starts_with("fono_"));
    let path = directory.0.join("tokens");
    assert!(!std::fs::read_to_string(&path).unwrap().contains(token));
    assert_eq!(
        FileTokenVerifier::open(&path).unwrap().scope(token),
        Some(TokenScope::Recognize)
    );
    assert_eq!(
        request(app.clone(), "GET", "/v1/capabilities", Some(token))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(app.clone(), "GET", "/v1/clients", Some(token))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    let listing = json(request(app.clone(), "GET", "/v1/clients", Some(&admin)).await).await;
    assert_eq!(listing["api_base_url"], "http://fono:17832");
    assert_eq!(listing["data"].as_array().unwrap().len(), 2);
    assert!(listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["scope"] == "recognize" && entry.as_object().unwrap().len() == 3));
    let encoded = serde_json::to_string(&listing).unwrap();
    for forbidden in [
        token,
        admin.as_str(),
        "operator",
        "digest",
        "salt",
        "tokens_file",
    ] {
        assert!(!encoded.contains(forbidden), "{forbidden}");
    }
    let before = std::fs::read(&path).unwrap();
    let duplicate = call(
        app.clone(),
        "POST",
        "/v1/clients",
        Some(&admin),
        Body::from(r#"{"name":"test-app"}"#),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(json(duplicate).await["error"]["code"], "client_exists");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let admin_revoke = request(app.clone(), "DELETE", "/v1/clients/operator", Some(&admin)).await;
    assert_eq!(admin_revoke.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json(admin_revoke).await["error"]["code"],
        "admin_client_protected"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);

    let revoked = request(app.clone(), "DELETE", "/v1/clients/test-app", Some(&admin)).await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert!(to_bytes(revoked.into_body(), 1024)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        request(app.clone(), "GET", "/v1/capabilities", Some(token))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(app.clone(), "DELETE", "/v1/clients/test-app", Some(&admin))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        FileTokenVerifier::open(path).unwrap().scope(&admin),
        Some(TokenScope::Admin)
    );
}

#[tokio::test]
async fn invalid_client_creations_are_bounded_and_never_mutate_the_store() {
    let (mut state, directory, admin, _client) = client_state();
    Arc::get_mut(&mut state)
        .unwrap()
        .config
        .server
        .body_read_timeout = Duration::from_millis(5);
    let app = test_router(state);
    let path = directory.0.join("tokens");
    let before = std::fs::read(&path).unwrap();
    for body in [
        r#"{"name":"invalid name"}"#,
        r#"{"name":"test-app","scope":"admin"}"#,
        r#"{"name":"test-app","name":"other"}"#,
        r#"{"name":42}"#,
        "{",
    ] {
        let response = call(
            app.clone(),
            "POST",
            "/v1/clients",
            Some(&admin),
            Body::from(body),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json(response).await["error"]["code"], "invalid_request");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let oversized = call(
        app.clone(),
        "POST",
        "/v1/clients",
        Some(&admin),
        Body::from(" ".repeat(1025)),
    )
    .await;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let pending = Body::from_stream(futures_util::stream::pending::<
        Result<&'static str, std::io::Error>,
    >());
    let timeout = call(app, "POST", "/v1/clients", Some(&admin), pending).await;
    assert_eq!(timeout.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(json(timeout).await["error"]["code"], "body_read_timeout");
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn client_listing_never_derives_an_advertised_url_from_request_headers() {
    let (mut state, _directory, admin, _client) = client_state();
    Arc::get_mut(&mut state).unwrap().config.server.client_url = None;
    let response = test_router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/clients")
                .header(header::AUTHORIZATION, format!("Bearer {admin}"))
                .header(header::HOST, "attacker.invalid:9999")
                .header("x-forwarded-host", "attacker.invalid")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json(response).await["api_base_url"].is_null());
}

#[tokio::test]
async fn credential_write_failure_never_returns_a_secret_or_storage_path() {
    let (mut state, directory, admin, _client) = client_state();
    let unavailable = directory.0.join("directory-not-a-token-file");
    std::fs::create_dir(&unavailable).unwrap();
    // Keep the verifier attached to the legitimate existing store while the
    // write path fails, modelling a store becoming unwritable after auth.
    Arc::get_mut(&mut state).unwrap().config.server.tokens_file = unavailable;
    let response = call(
        test_router(state),
        "POST",
        "/v1/clients",
        Some(&admin),
        Body::from(r#"{"name":"test-app"}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let value = json(response).await;
    assert_eq!(value["error"]["code"], "client_store_unavailable");
    let encoded = serde_json::to_string(&value).unwrap();
    assert!(
        !encoded.contains("fono_")
            && !encoded.contains("directory-not-a-token-file")
            && !encoded.contains(&admin)
    );
}
