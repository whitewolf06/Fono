//! Protocol compatibility is independent from application/worker versions.
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{failure, transport::Transport, wire, RemoteError, RemoteErrorKind, PCM_TYPE};
use crate::error::AppResult;
use fono_types::SttEngine;

pub const MIN_SERVER_API_VERSION: u16 = 2;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Interactive,
    Batch,
}

pub(super) struct InteractiveRequest<'a> {
    pub samples: usize,
    pub language: &'a str,
    pub decoding: &'a str,
    pub timestamps: bool,
    pub glossary: bool,
}

pub(super) fn validate_request(
    server: &wire::Capabilities,
    model: &wire::Details,
    request: InteractiveRequest<'_>,
) -> AppResult<()> {
    let max_samples = server
        .max_audio_seconds
        .saturating_mul(16_000)
        .min(server.max_upload_bytes / 2);
    if request.samples as u64 > max_samples
        || !model
            .languages
            .iter()
            .any(|language| language == request.language)
    {
        return failure(RemoteErrorKind::InvalidRequest);
    }
    if !server
        .features
        .decoding
        .iter()
        .any(|mode| mode == request.decoding)
        || !model
            .features
            .decoding
            .iter()
            .any(|mode| mode == request.decoding)
        || (request.timestamps
            && (!server
                .features
                .timestamps
                .iter()
                .any(|mode| mode == "segments")
                || !model
                    .features
                    .timestamps
                    .iter()
                    .any(|mode| mode == "segments")))
        || (request.glossary && !(server.features.glossary && model.features.glossary))
    {
        return failure(RemoteErrorKind::UnsupportedCapabilities);
    }
    Ok(())
}

/// Read the small version envelope before deserializing any v2-only fields.
/// API 1 may legitimately lack features, devices and other v2 metadata.
pub(super) async fn description<T: DeserializeOwned>(http: &Transport) -> AppResult<T> {
    let value = http
        .json::<Value>(http.get("v1/capabilities"), 64 * 1024)
        .await
        .map_err(|error| match error {
            crate::error::SttError::Remote(mut error) if error.status == Some(404) => {
                error.kind = RemoteErrorKind::Incompatible;
                error.into()
            }
            other => other,
        })?;
    if value
        .get("product")
        .and_then(Value::as_str)
        .is_some_and(|product| product != "fono-server")
    {
        return failure(RemoteErrorKind::Incompatible);
    }
    let version = value
        .get("api_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| RemoteError::new(RemoteErrorKind::InvalidResponse))?;
    if version < u64::from(MIN_SERVER_API_VERSION)
        || value.get("product").and_then(Value::as_str) != Some("fono-server")
    {
        return failure(RemoteErrorKind::Incompatible);
    }
    if value
        .get("features")
        .is_some_and(|features| !features.is_object())
    {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    serde_json::from_value(value)
        .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidResponse).into())
}

pub(super) async fn models(http: &Transport) -> AppResult<wire::Models> {
    let value = http
        .json::<Value>(http.get("v1/models"), 512 * 1024)
        .await?;
    if value
        .get("data")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                model
                    .get("fono")
                    .and_then(|details| details.get("features"))
                    .is_some_and(|features| !features.is_object())
            })
        })
    {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    serde_json::from_value(value)
        .map_err(|_| RemoteError::new(RemoteErrorKind::InvalidResponse).into())
}

pub(super) fn validate_server(server: &wire::Capabilities, route: Route) -> AppResult<()> {
    if server.product != "fono-server" || server.api_version < MIN_SERVER_API_VERSION {
        return failure(RemoteErrorKind::Incompatible);
    }
    let supported = match route {
        Route::Interactive => {
            server.audio_formats.iter().any(|format| format == PCM_TYPE)
                && server.features.cancel_on_disconnect
                && server.features.decoding.iter().any(|mode| mode == "fast")
                && server.max_upload_bytes >= 2
        }
        Route::Batch => {
            server
                .audio_formats
                .iter()
                .any(|format| format == "audio/wav")
                && server.features.jobs
                && server.max_upload_bytes > 44
        }
    };
    if !supported || server.max_audio_seconds == 0 {
        return failure(RemoteErrorKind::UnsupportedCapabilities);
    }
    Ok(())
}

pub(super) fn validate_details(details: &wire::Details) -> AppResult<()> {
    if !matches!(
        details.state.as_str(),
        "ready" | "busy" | "loading" | "retiring" | "unloaded" | "failed"
    ) || !matches!(
        details.disk_state.as_str(),
        "verified" | "present_unverified" | "missing" | "invalid"
    ) || details.languages.len() > 64
        || details.languages.iter().any(|language| {
            language.is_empty()
                || language.len() > 16
                || !language
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        })
    {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    wire::device(&details.device)?;
    Ok(())
}

/// Called both at connection and immediately before an audio request. A server
/// restart at the same endpoint must not reuse stale advertised capabilities.
pub(super) async fn discover(
    http: &Transport,
    model: &str,
    engine: SttEngine,
    route: Route,
) -> AppResult<(wire::Capabilities, wire::Details)> {
    let server: wire::Capabilities = description(http).await?;
    validate_server(&server, route)?;
    let models = models(http).await?;
    if models.object != "list" || models.data.len() > 256 {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    let mut matches = models
        .data
        .into_iter()
        .filter(|candidate| candidate.id == model);
    let Some(selected) = matches.next() else {
        return failure(RemoteErrorKind::ModelNotFound);
    };
    if matches.next().is_some() || selected.object != "model" || selected.fono.engine != engine {
        return failure(RemoteErrorKind::InvalidResponse);
    }
    let details = selected.fono;
    validate_details(&details)?;
    if !matches!(
        details.disk_state.as_str(),
        "verified" | "present_unverified"
    ) || details.state == "failed"
    {
        return failure(RemoteErrorKind::ModelNotReady);
    }
    if details.languages.is_empty()
        || (matches!(route, Route::Interactive)
            && !details.features.decoding.iter().any(|mode| mode == "fast"))
    {
        return failure(RemoteErrorKind::UnsupportedCapabilities);
    }
    Ok((server, details))
}
