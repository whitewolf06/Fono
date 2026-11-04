//! The login shell is public; every data request still uses the existing bearer API.
//! Generated Vue/WhiteUI assets are embedded so Cargo/Docker need no Node runtime.

use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use fono_service::rest_api::RestApiState;

const INDEX: &[u8] = include_bytes!("../resources/panel/index.html");
const SCRIPT: &[u8] = include_bytes!("../resources/panel/panel.js");
const STYLE: &[u8] = include_bytes!("../resources/panel/panel.css");
const POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";

pub fn routes() -> Router<RestApiState> {
    Router::new()
        .route("/", get(index))
        .route("/panel", get(index))
        .route("/panel/", get(index))
        .route("/panel/panel.js", get(script))
        .route("/panel/panel.css", get(style))
}

pub fn response(body: &'static [u8], content_type: &'static str) -> Response {
    let mut response = ([(header::CONTENT_TYPE, content_type)], body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}
async fn index() -> Response {
    response(INDEX, "text/html; charset=utf-8")
}
async fn script() -> Response {
    response(SCRIPT, "text/javascript; charset=utf-8")
}
async fn style() -> Response {
    response(STYLE, "text/css; charset=utf-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn embedded_panel_is_public_self_contained_and_has_security_headers() {
        // This router contains only public assets, never API data or credentials.
        let app = Router::new()
            .route("/", get(index))
            .route("/panel/panel.js", get(script))
            .route("/panel/panel.css", get(style));
        for (path, content_type) in [
            ("/", "text/html"),
            ("/panel/panel.js", "text/javascript"),
            ("/panel/panel.css", "text/css"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with(content_type));
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
            assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
            assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], POLICY);
            assert!(!response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
            assert!(!to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .is_empty());
        }
        let html = std::str::from_utf8(INDEX).unwrap();
        assert!(html.contains("/panel/panel.js"));
        assert!(html.contains("/panel/panel.css"));
        assert!(!html.contains("https://"));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/panel/manifest.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
