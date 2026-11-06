//! Administrator-only provisioning for recognition clients. Secrets are
//! returned once after durable publication; inventory contains metadata only.

use axum::body::to_bytes;
use serde::Deserialize;

use super::*;
use crate::tokens::{self, TokenError, TokenSummary};

const MAX_CREATE_BYTES: usize = 1024;

#[derive(Serialize)]
struct Client {
    name: String,
    scope: &'static str,
    created_at: String,
}

impl From<TokenSummary> for Client {
    fn from(value: TokenSummary) -> Self {
        Self {
            name: value.name,
            scope: "recognize",
            created_at: value.created_at,
        }
    }
}

#[derive(Serialize)]
struct ClientList {
    data: Vec<Client>,
    api_base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateClient {
    name: String,
}

#[derive(Serialize)]
struct CreatedClient {
    #[serde(flatten)]
    client: Client,
    token: String,
}

fn invalid() -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_request",
        message: "Provide a client name using 1-64 ASCII letters, digits, _ or -",
        retryable: false,
    }
}

fn store_error(error: TokenError) -> ApiError {
    let (status, code, message) = match error {
        TokenError::InvalidName => return invalid(),
        TokenError::DuplicateName => (
            StatusCode::CONFLICT,
            "client_exists",
            "A client with this name already exists",
        ),
        TokenError::NotFound => (
            StatusCode::NOT_FOUND,
            "client_not_found",
            "The client does not exist",
        ),
        TokenError::ProtectedAdmin => (
            StatusCode::FORBIDDEN,
            "admin_client_protected",
            "Admin credentials cannot be revoked through the clients API",
        ),
        TokenError::Full => (
            StatusCode::CONFLICT,
            "client_limit",
            "The credential store has reached its client or size limit",
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            "client_store_unavailable",
            "The credential store is unavailable or cannot be updated",
        ),
    };
    ApiError {
        status,
        code,
        message,
        retryable: false,
    }
}

pub(super) async fn list(
    Extension(state): Extension<Arc<ApiState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Admin).await {
        return error.response(&headers);
    }
    let api_base_url = state.config.server.client_url.clone();
    let path = state.config.server.tokens_file.clone();
    match tokio::task::spawn_blocking(move || tokens::list_clients(&path)).await {
        Ok(Ok(entries)) => Json(ClientList {
            data: entries.into_iter().map(Client::from).collect(),
            api_base_url,
        })
        .into_response(),
        Ok(Err(error)) => store_error(error).response(&headers),
        Err(_) => ApiError::internal().response(&headers),
    }
}

pub(super) async fn create(
    Extension(state): Extension<Arc<ApiState>>,
    request: Request,
) -> Response {
    let headers = request.headers().clone();
    // Authenticate before reading even one body byte. No automatic JSON
    // extractor may allocate/parse an unauthenticated request body.
    if let Err(error) = authorize(&state, &headers, TokenScope::Admin).await {
        return error.response(&headers);
    }
    let content_types: Vec<_> = headers.get_all(header::CONTENT_TYPE).iter().collect();
    if content_types.len() != 1
        || !content_types[0].to_str().ok().is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return invalid().response(&headers);
    }
    let body = match tokio::time::timeout(
        state.config.server.body_read_timeout,
        to_bytes(request.into_body(), MAX_CREATE_BYTES),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => {
            return ApiError {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "body_too_large",
                message: "Client creation JSON must not exceed 1024 bytes",
                retryable: false,
            }
            .response(&headers)
        }
        Err(_) => {
            return ApiError {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "body_read_timeout",
                message: "Request body deadline exceeded",
                retryable: false,
            }
            .response(&headers)
        }
    };
    let Ok(input) = serde_json::from_slice::<CreateClient>(&body) else {
        return invalid().response(&headers);
    };
    let activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(error) => return command_error(error).response(&headers),
    };
    let path = state.config.server.tokens_file.clone();
    match tokio::task::spawn_blocking(move || {
        // HTTP cancellation must not release shutdown admission while a
        // durable atomic write is still running.
        let _activity = activity;
        tokens::create_client(&path, &input.name)
    })
    .await
    {
        Ok(Ok((client, token))) => (
            StatusCode::CREATED,
            Json(CreatedClient {
                client: client.into(),
                token,
            }),
        )
            .into_response(),
        Ok(Err(error)) => store_error(error).response(&headers),
        Err(_) => ApiError::internal().response(&headers),
    }
}

pub(super) async fn revoke(
    Extension(state): Extension<Arc<ApiState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if let Err(error) = authorize(&state, &headers, TokenScope::Admin).await {
        return error.response(&headers);
    }
    let activity = match state.activity.lease() {
        Ok(activity) => activity,
        Err(error) => return command_error(error).response(&headers),
    };
    let path = state.config.server.tokens_file.clone();
    match tokio::task::spawn_blocking(move || {
        let _activity = activity;
        tokens::revoke_client(&path, &name)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => store_error(error).response(&headers),
        Err(_) => ApiError::internal().response(&headers),
    }
}
