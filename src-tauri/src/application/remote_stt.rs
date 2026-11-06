//! Explicit connection tests and OS credential operations for remote settings.
use crate::{
    error::{AppError, AppResult},
    types::{RemoteSshSettings, RemoteSttSettings},
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
    pub ssh: Option<RemoteSshSettings>,
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
    SshInvalidConfiguration,
    SshClientMissing,
    SshIdentityMissing,
    SshHostKeyRequired,
    SshConnectionFailed,
    SshBusy,
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
    if let Some(ssh) = &request.ssh {
        super::ssh_tunnel::validate(ssh).map_err(ssh_connection_error)?;
    }
    let token = if let Some(token) = request.token {
        token
    } else {
        if !saved.is_some_and(|saved| {
            matches_saved_endpoint(saved, &request.url, request.ca_pem.as_deref())
                && saved.ssh == request.ssh
        }) {
            return Err(ConnectionError::NotConfigured);
        }
        load_token()
            .map_err(|_| ConnectionError::NotConfigured)?
            .map(Zeroizing::new)
            .filter(|token| !token.is_empty())
            .ok_or(ConnectionError::NotConfigured)?
    };
    // A draft owns an isolated relay. It cannot replace the saved connection's
    // listener or retire a recognizer that might still be in use.
    let tunnel = request
        .ssh
        .as_ref()
        .map(super::ssh_tunnel::preview)
        .transpose()
        .map_err(ssh_connection_error)?;
    let url = tunnel
        .as_ref()
        .map(|tunnel| tunnel.url())
        .unwrap_or(&request.url);
    let endpoint = endpoint(url, &token, request.ca_pem.as_deref()).map_err(map_error)?;
    inspect_server(
        endpoint,
        request.model.as_ref(),
        Arc::new(crate::stt::DesktopActivity),
        &OperationCancellation::default(),
    )
    .map_err(|error| {
        let classified = map_error(error);
        if tunnel.is_some()
            && matches!(
                classified,
                ConnectionError::Network | ConnectionError::Timeout
            )
        {
            ConnectionError::SshConnectionFailed
        } else {
            classified
        }
    })
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
        ssh: remote.ssh.clone(),
        // Catalog refresh must still offer replacements if the saved model was removed.
        model: None,
    }
}

pub(crate) fn ssh_connection_error(error: super::ssh_tunnel::SshError) -> ConnectionError {
    use super::ssh_tunnel::SshError as S;
    match error {
        S::InvalidConfiguration => ConnectionError::SshInvalidConfiguration,
        S::ClientMissing => ConnectionError::SshClientMissing,
        S::IdentityMissing => ConnectionError::SshIdentityMissing,
        S::HostKeyRequired => ConnectionError::SshHostKeyRequired,
        S::ConnectionFailed => ConnectionError::SshConnectionFailed,
        S::Busy => ConnectionError::SshBusy,
    }
}

pub(crate) fn ssh_app_error(error: super::ssh_tunnel::SshError) -> AppError {
    let kind = match error {
        super::ssh_tunnel::SshError::Busy => RemoteErrorKind::Busy,
        super::ssh_tunnel::SshError::ConnectionFailed => RemoteErrorKind::Network,
        _ => RemoteErrorKind::InvalidEndpoint,
    };
    fono_stt::error::SttError::Remote(fono_stt::remote::RemoteError {
        kind,
        status: None,
        retry_after_seconds: None,
    })
    .into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostKeyRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum HostKeyResult {
    Ok {
        host_key: String,
        fingerprint: String,
    },
    Error {
        error: ConnectionError,
    },
}

pub(crate) fn probe_host_key(request: HostKeyRequest) -> HostKeyResult {
    match super::ssh_tunnel::probe_host_key(&request.host, request.port) {
        Ok(key) => HostKeyResult::Ok {
            host_key: key.host_key,
            fingerprint: key.fingerprint,
        },
        Err(error) => HostKeyResult::Error {
            error: ssh_connection_error(error),
        },
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
            ssh: None,
            fallback: Default::default(),
        }
    }
    fn request(url: &str) -> TestRequest {
        TestRequest {
            url: url.into(),
            token: None,
            ca_pem: None,
            ssh: None,
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

    fn ssh_fixture() -> RemoteSshSettings {
        use base64::Engine;
        let mut blob = Vec::new();
        blob.extend_from_slice(&11u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-ed25519");
        blob.extend_from_slice(&32u32.to_be_bytes());
        blob.extend_from_slice(&[7u8; 32]);
        RemoteSshSettings {
            host: "server.example".into(),
            port: 22,
            user: "test-user".into(),
            identity_file: None,
            server_port: 17832,
            host_key: format!(
                "ssh-ed25519 {}",
                base64::engine::general_purpose::STANDARD.encode(blob)
            ),
        }
    }

    #[test]
    fn changed_ssh_identity_cannot_read_or_forward_the_saved_bearer() {
        let mut saved = remote();
        saved.ssh = Some(ssh_fixture());
        for edit in 0..6 {
            let mut changed = request(&saved.url);
            let mut ssh = ssh_fixture();
            match edit {
                0 => ssh.host = "other.example".into(),
                1 => ssh.port = 2222,
                2 => ssh.user = "other-user".into(),
                3 => ssh.server_port = 17834,
                4 => ssh.identity_file = Some(std::env::temp_dir().join("other-identity")),
                _ => ssh.host_key = ssh.host_key.replace("Bwc", "CAg"),
            }
            changed.ssh = Some(ssh);
            assert_ne!(changed.ssh, saved.ssh);
            let result = test_with_credential(changed, Some(&saved), || {
                panic!("changed SSH destination must not read bearer")
            });
            assert!(matches!(result, Err(ConnectionError::NotConfigured)));
        }
        assert!(matches!(
            test_with_credential(request(&saved.url), Some(&saved), || panic!(
                "SSH to direct is a different destination"
            )),
            Err(ConnectionError::NotConfigured)
        ));
    }

    #[test]
    fn legacy_connections_remain_direct_and_ssh_configuration_roundtrips_without_secrets() {
        let original = remote();
        let json = serde_json::to_value(&original).unwrap();
        assert!(json.get("ssh").is_none());
        let old: RemoteSttSettings = serde_json::from_value(json).unwrap();
        assert_eq!(old.ssh, None);
        let mut configured = old;
        configured.ssh = Some(ssh_fixture());
        let encoded = serde_json::to_string(&configured).unwrap();
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("passphrase"));
        assert_eq!(
            serde_json::from_str::<RemoteSttSettings>(&encoded).unwrap(),
            configured
        );
    }

    #[test]
    #[ignore = "requires an explicitly supplied trusted SSH config and API credential file"]
    fn real_managed_ssh_kari_roundtrip() {
        use fono_stt::remote::{RemoteRecognizer, RemoteTranscriptionClient};
        use sha2::{Digest, Sha256};
        let config_file =
            std::env::var_os("FONO_SSH_SMOKE_CONFIG").expect("explicit SSH fixture is required");
        let token_file = std::env::var_os("FONO_SSH_SMOKE_TOKEN_FILE")
            .expect("explicit API credential file is required");
        let mut ssh: RemoteSshSettings =
            serde_json::from_slice(&std::fs::read(config_file).expect("cannot read SSH fixture"))
                .expect("invalid trusted SSH fixture");
        // Exercise the real first-use path, not only a pre-supplied pin. The
        // observed candidate still needs independent trust before API traffic.
        let observed = super::super::ssh_tunnel::probe_host_key(&ssh.host, ssh.port)
            .expect("production host-key retrieval failed");
        assert_eq!(
            observed.host_key, ssh.host_key,
            "observed host key differs from the independently trusted fixture"
        );
        ssh.host_key = observed.host_key;
        let token = Zeroizing::new(
            std::fs::read_to_string(token_file).expect("cannot read API credential fixture"),
        );
        let started = std::time::Instant::now();
        let result = test_with_credential(
            TestRequest {
                url: "http://127.0.0.1:17832".into(),
                token: Some(Zeroizing::new(token.trim().to_owned())),
                ca_pem: None,
                ssh: Some(ssh.clone()),
                model: None,
            },
            None,
            || panic!("explicit test token must not read the OS credential"),
        );
        let server = match result {
            Ok(server) => server,
            Err(error) => panic!("managed SSH metadata failed: {error:?}"),
        };
        let saved = super::super::ssh_tunnel::resolve_saved(&ssh)
            .expect("cannot resolve saved SSH configuration");
        assert_eq!(
            saved,
            super::super::ssh_tunnel::resolve_saved(&ssh).unwrap()
        );
        let wav = std::env::var_os("FONO_SSH_SMOKE_WAV")
            .expect("explicit public WAV fixture is required");
        let wav_bytes = std::fs::read(&wav).expect("cannot read the public WAV fixture");
        assert_eq!(
            format!("{:x}", Sha256::digest(wav_bytes)),
            "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e"
        );
        let audio =
            fono_service::audio_ingest::decode_file(std::path::Path::new(&wav), Default::default())
                .expect("cannot decode public WAV fixture");
        let model = ModelId::whisper(WhisperModelSize::LargeTurbo);
        assert!(
            server
                .models
                .iter()
                .any(|entry| entry.id == model && entry.installed),
            "the explicit public smoke model must be installed"
        );
        let endpoint = RemoteEndpoint::new(&saved, token.trim()).unwrap();
        let cancellation = OperationCancellation::default();
        let client = RemoteRecognizer::connect(
            endpoint.clone(),
            model.clone(),
            Arc::new(crate::stt::DesktopActivity),
            &cancellation,
        )
        .unwrap_or_else(|_| panic!("managed SSH interactive connection failed"));
        let transcript = client
            .transcribe(
                &audio.pcm_samples,
                fono_stt::RecognitionOptions {
                    language: "en",
                    decoding: Default::default(),
                    glossary: &[],
                },
                &cancellation,
            )
            .unwrap_or_else(|_| panic!("managed SSH interactive transcription failed"));
        let batch = RemoteTranscriptionClient::connect(
            endpoint,
            model,
            Arc::new(crate::stt::DesktopActivity),
            &cancellation,
        )
        .unwrap_or_else(|_| panic!("managed SSH file connection failed"))
        .transcribe_pcm_job(&audio.pcm_samples, "en", &cancellation, &|_| {})
        .unwrap_or_else(|_| panic!("managed SSH file transcription failed"));
        for text in [&transcript.text, &batch.text] {
            let normalized = text.to_lowercase();
            for expected in ["fellow", "americans", "ask", "country"] {
                assert!(
                    normalized
                        .split(|character: char| !character.is_ascii_alphabetic())
                        .any(|word| word == expected),
                    "public speech reference word is missing"
                );
            }
        }
        assert!(transcript.remote.is_some());
        assert!(batch
            .backend
            .as_deref()
            .is_some_and(|backend| backend.starts_with("remote:")));
        super::super::ssh_tunnel::shutdown();
        assert!(std::net::TcpStream::connect(
            saved.trim_start_matches("http://").trim_end_matches('/')
        )
        .is_err());
        println!(
            "{}",
            serde_json::json!({ "status":"passed", "api_version":server.api_version, "models":server.models.len(), "devices":server.devices.len(), "elapsed_ms":started.elapsed().as_millis(), "audio_samples":audio.pcm_samples.len(), "reference_words_matched":4, "interactive_backend":transcript.device, "batch_backend":batch.backend })
        );
    }
}
