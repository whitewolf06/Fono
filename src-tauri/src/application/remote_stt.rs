//! Explicit connection tests and OS credential operations for remote settings.
use crate::{
    error::{AppError, AppResult},
    types::RemoteSttSettings,
};
use fono_core::OperationCancellation;
use fono_stt::remote::{inspect_server, RemoteEndpoint, RemoteErrorKind, RemoteInspection};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestRequest {
    pub url: String,
    #[serde(default, deserialize_with = "secret")]
    token: Option<Zeroizing<String>>,
    #[serde(default)]
    pub ca_pem: Option<String>,
    #[serde(default)]
    pub model: Option<crate::types::ModelId>,
}
fn secret<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|value| value.map(Zeroizing::new))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectionError {
    InvalidEndpoint,
    NotConfigured,
    Network,
    Timeout,
    Unauthorized,
    IncompatibleVersion,
    UnsupportedCapabilities,
    ModelNotFound,
    InvalidResponse,
    Busy,
    ServerFailed,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TestResult {
    Ok { server: RemoteInspection },
    Error { error: ConnectionError },
}
#[derive(Serialize)]
pub(crate) struct CredentialStatus {
    pub has_token: bool,
}

pub(crate) fn credential_status() -> AppResult<CredentialStatus> {
    let token = crate::secrets::load_stt_server_token()?.map(Zeroizing::new);
    Ok(CredentialStatus {
        has_token: token.as_ref().is_some_and(|token| !token.is_empty()),
    })
}

pub(crate) fn test(request: TestRequest, saved: Option<&RemoteSttSettings>) -> TestResult {
    match test_with_credential(request, saved, crate::secrets::load_stt_server_token) {
        Ok(server) => TestResult::Ok { server },
        Err(error) => TestResult::Error { error },
    }
}

fn test_with_credential(
    request: TestRequest,
    saved: Option<&RemoteSttSettings>,
    load_token: impl FnOnce() -> AppResult<Option<String>>,
) -> Result<RemoteInspection, ConnectionError> {
    // Validate the address before requesting any stored secret. A draft address
    // cannot cause an ambient credential to be sent to a different server.
    endpoint(
        &request.url,
        "configuration-validation",
        request.ca_pem.as_deref(),
    )
    .map_err(map_error)?;
    let token = if let Some(token) = request.token {
        token
    } else {
        if !saved.is_some_and(|saved| {
            matches_saved_endpoint(saved, &request.url, request.ca_pem.as_deref())
        }) {
            return Err(ConnectionError::NotConfigured);
        }
        load_token()
            .map_err(|_| ConnectionError::NotConfigured)?
            .map(Zeroizing::new)
            .filter(|token| !token.is_empty())
            .ok_or(ConnectionError::NotConfigured)?
    };
    let endpoint = endpoint(&request.url, &token, request.ca_pem.as_deref()).map_err(map_error)?;
    inspect_server(
        endpoint,
        request.model.as_ref(),
        Arc::new(crate::stt::DesktopActivity),
        &OperationCancellation::default(),
    )
    .map_err(map_error)
}

fn endpoint(
    url: &str,
    token: &str,
    ca: Option<&str>,
) -> fono_stt::error::SttResult<RemoteEndpoint> {
    let endpoint = RemoteEndpoint::new(url, token)?;
    match ca.filter(|ca| !ca.trim().is_empty()) {
        Some(ca) => endpoint.with_ca_pem(ca.as_bytes()),
        None => Ok(endpoint),
    }
}
fn matches_saved_endpoint(saved: &RemoteSttSettings, url: &str, ca: Option<&str>) -> bool {
    match (reqwest::Url::parse(&saved.url), reqwest::Url::parse(url)) {
        (Ok(saved_url), Ok(url)) => {
            saved_url == url
                && saved.ca_pem.as_deref().unwrap_or("").trim() == ca.unwrap_or("").trim()
        }
        _ => false,
    }
}
fn map_error(error: fono_stt::error::SttError) -> ConnectionError {
    use ConnectionError as C;
    use RemoteErrorKind as R;
    match error {
        fono_stt::error::SttError::Remote(error) => match error.kind {
            R::InvalidEndpoint | R::InvalidRequest => C::InvalidEndpoint,
            R::Unauthorized => C::Unauthorized,
            R::Incompatible => C::IncompatibleVersion,
            R::UnsupportedCapabilities => C::UnsupportedCapabilities,
            R::ModelNotFound => C::ModelNotFound,
            R::Busy => C::Busy,
            R::Timeout => C::Timeout,
            R::Network => C::Network,
            R::InvalidResponse => C::InvalidResponse,
            _ => C::ServerFailed,
        },
        _ => C::ServerFailed,
    }
}

pub(crate) fn saved_request(remote: &RemoteSttSettings) -> TestRequest {
    TestRequest {
        url: remote.url.clone(),
        token: None,
        ca_pem: remote.ca_pem.clone(),
        // Catalog refresh must still offer replacements if the saved model was removed.
        model: None,
    }
}

/// The caller owns updater admission and the settings transaction until this
/// blocking action ends. Capture starts share the configuration reservation.
pub(crate) fn change_token(app: &AppHandle, token: Option<&str>) -> AppResult<()> {
    if let Some(token) = token {
        RemoteEndpoint::new("https://credential-validation.invalid", token)
            .map_err(|_| AppError::Config("Invalid Fono Server token".into()))?;
    }
    let _configuration = super::capture_configuration::reserve_configuration(|| {
        let pipeline = app.state::<crate::pipeline::Pipeline>();
        let queue = app
            .state::<super::service_control::ServiceControl>()
            .snapshot()
            .snapshot
            .queue;
        ensure_credential_idle(
            super::dictation::workflow::is_busy(app)
                || pipeline.current_operation().is_some()
                || pipeline.has_active_service_operation()
                || pipeline.scheduler().is_busy()
                || queue.queued + queue.preparing + queue.transcribing > 0,
        )
    })?;
    // Do not retain an authenticated old remote client after credential change.
    retire_then_write(
        || {
            app.state::<crate::pipeline::Pipeline>()
                .stt()
                .clear_remote_if_idle()
        },
        || match token {
            Some(token) => crate::secrets::store_stt_server_token(token),
            None => crate::secrets::delete_stt_server_token(),
        },
    )
}

fn ensure_credential_idle(active: bool) -> AppResult<()> {
    if active {
        return Err(AppError::Busy(
            "Сначала завершите диктовку и задачи распознавания".into(),
        ));
    }
    Ok(())
}

fn retire_then_write(
    retire: impl FnOnce() -> AppResult<()>,
    write: impl FnOnce() -> AppResult<()>,
) -> AppResult<()> {
    retire()?;
    write()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelId, WhisperModelSize};
    fn remote() -> RemoteSttSettings {
        RemoteSttSettings {
            url: "https://server.example".into(),
            model: ModelId::whisper(WhisperModelSize::Tiny),
            display_name: Some("Kari".into()),
            ca_pem: None,
            fallback: Default::default(),
        }
    }
    fn request(url: &str) -> TestRequest {
        TestRequest {
            url: url.into(),
            token: None,
            ca_pem: None,
            model: None,
        }
    }

    #[test]
    fn active_capture_and_failed_client_retirement_cannot_change_credentials() {
        let pipeline = crate::pipeline::Pipeline::new();
        let capture = pipeline
            .reserve_capture(fono_core::OperationSource::Ui)
            .unwrap();
        assert!(matches!(
            ensure_credential_idle(pipeline.current_operation().is_some()),
            Err(AppError::Busy(_))
        ));
        pipeline.cancel_for(capture).unwrap();
        let operation = pipeline.start_service_transcription().unwrap().0;
        assert!(matches!(
            ensure_credential_idle(pipeline.has_active_service_operation()),
            Err(AppError::Busy(_))
        ));
        pipeline.finish_service_operation(operation, fono_core::TerminalReason::Cancelled);
        assert!(ensure_credential_idle(pipeline.has_active_service_operation()).is_ok());
        let result = retire_then_write(
            || Err(AppError::Busy("still retiring".into())),
            || panic!("previous credential must remain unchanged"),
        );
        assert!(matches!(result, Err(AppError::Busy(_))));
        let result = retire_then_write(
            || Ok(()),
            || Err(AppError::Config("OS write rejected".into())),
        );
        assert!(matches!(result, Err(AppError::Config(_))));
    }

    #[test]
    fn changed_endpoint_or_ca_never_reads_an_ambient_credential() {
        let saved = remote();
        for url in [
            "https://different.example",
            "http://192.168.1.8:17832",
            "https://server.example?secret=x",
        ] {
            let result = test_with_credential(request(url), Some(&saved), || {
                panic!("must not read credentials for draft endpoint")
            });
            assert!(matches!(
                result,
                Err(ConnectionError::NotConfigured | ConnectionError::InvalidEndpoint)
            ));
        }
        let mut changed = request(&saved.url);
        changed.ca_pem = Some("invalid CA".into());
        assert!(
            test_with_credential(changed, Some(&saved), || panic!("must not read credential"))
                .is_err()
        );
        assert!(matches_saved_endpoint(
            &saved,
            "https://server.example/",
            None
        ));
        assert!(!matches_saved_endpoint(&saved, &saved.url, Some("new CA")));
    }
    #[test]
    fn missing_saved_credential_and_errors_are_typed_without_sensitive_values() {
        let saved = remote();
        assert!(matches!(
            test_with_credential(request(&saved.url), Some(&saved), || Ok(None)),
            Err(ConnectionError::NotConfigured)
        ));
        let error = TestResult::Error {
            error: map_error(fono_stt::error::SttError::Config(
                "PRIVATE_URL_TOKEN_SERVER_BODY".into(),
            )),
        };
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({"kind":"error","error":"server_failed"})
        );
        let status = serde_json::to_value(CredentialStatus { has_token: true }).unwrap();
        assert_eq!(status, serde_json::json!({"has_token":true}));
    }

    #[test]
    fn protocol_version_and_capability_failures_have_distinct_public_codes() {
        use fono_stt::remote::RemoteError;
        for (kind, expected) in [
            (RemoteErrorKind::Incompatible, "incompatible_version"),
            (
                RemoteErrorKind::UnsupportedCapabilities,
                "unsupported_capabilities",
            ),
        ] {
            let result = TestResult::Error {
                error: map_error(fono_stt::error::SttError::Remote(RemoteError {
                    kind,
                    status: Some(422),
                    retry_after_seconds: Some(2),
                })),
            };
            assert_eq!(
                serde_json::to_value(result).unwrap(),
                serde_json::json!({"kind":"error","error":expected})
            );
        }
    }
}
