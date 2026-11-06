//! Metadata-only preflight never owns capture or recognition completion.
use crate::{
    pipeline::{
        remote_check::{RemoteCheckReason as Reason, RemoteCheckState as State},
        Pipeline,
    },
    types::{Settings, SttSource},
};
use fono_core::OperationCancellation;
use fono_stt::{
    error::SttError,
    remote::{RemoteErrorKind, RemoteHealth, RemoteModelHealth, RemoteServerHealth},
};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Called after physical capture starts, before resolving focus/overlay. The
/// operation and its own cancellation token exist before work is queued.
pub(super) fn start(app: &AppHandle, operation: u64, settings: &Settings) {
    if settings.stt_source != SttSource::FonoServer {
        return;
    }
    let Ok(activity) = crate::application::updates::activity::lease() else {
        return;
    };
    let pipeline = app.state::<Pipeline>();
    let Some(cancellation) = pipeline.begin_remote_check_and_publish_for(operation, |snapshot| {
        crate::events::emit_remote_server_check(app, snapshot.clone());
    }) else {
        return;
    };
    let settings = settings.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        // The waiter can disappear; actual credential/HTTP work retains admission.
        let _activity = activity;
        probe(&settings, &cancellation)
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = task.await.unwrap_or(State::Unavailable {
            reason: Reason::InvalidResponse,
        });
        app.state::<Pipeline>()
            .complete_remote_check_and_publish_for(operation, state, |snapshot| {
                crate::events::emit_remote_server_check(&app, snapshot.clone());
            });
    });
}

fn probe(settings: &Settings, cancellation: &OperationCancellation) -> State {
    if cancellation.is_cancelled() {
        return State::Cancelled;
    }
    let Some(remote) = settings.remote_stt.as_ref() else {
        return State::Unavailable {
            reason: Reason::NotConfigured,
        };
    };
    let endpoint = match crate::stt::remote_endpoint(remote) {
        Ok(endpoint) => endpoint,
        Err(_) => {
            return State::Unavailable {
                reason: Reason::NotConfigured,
            }
        }
    };
    classify(fono_stt::remote::probe_health(
        endpoint,
        &remote.model,
        Arc::new(crate::stt::DesktopActivity),
        cancellation,
    ))
}

fn classify(result: fono_stt::error::SttResult<RemoteHealth>) -> State {
    match result {
        Ok(health) => match health.server {
            RemoteServerHealth::Starting => State::Starting,
            RemoteServerHealth::Stopping => State::Stopping,
            RemoteServerHealth::Failed => State::ModelNotReady,
            RemoteServerHealth::Ready => match health.selected_model {
                RemoteModelHealth::Busy => State::Busy,
                RemoteModelHealth::Loading
                | RemoteModelHealth::Failed
                | RemoteModelHealth::Unloaded => State::ModelNotReady,
                // Health lists preloads only. An omitted on-demand model is
                // not absent; recognition still makes its own authoritative attempt.
                RemoteModelHealth::Ready | RemoteModelHealth::Unknown => State::Ready,
            },
        },
        Err(SttError::Cancelled(_)) => State::Cancelled,
        Err(SttError::Remote(error)) => match error.kind {
            RemoteErrorKind::Busy => State::Busy,
            RemoteErrorKind::Timeout => State::Unavailable {
                reason: Reason::Timeout,
            },
            RemoteErrorKind::Unauthorized => State::Unavailable {
                reason: Reason::Unauthorized,
            },
            RemoteErrorKind::Incompatible => State::Unavailable {
                reason: Reason::IncompatibleVersion,
            },
            RemoteErrorKind::UnsupportedCapabilities => State::Unavailable {
                reason: Reason::UnsupportedCapabilities,
            },
            RemoteErrorKind::ModelNotFound
            | RemoteErrorKind::ModelNotReady
            | RemoteErrorKind::WorkerFailed => State::ModelNotReady,
            RemoteErrorKind::InvalidEndpoint | RemoteErrorKind::InvalidRequest => {
                State::Unavailable {
                    reason: Reason::NotConfigured,
                }
            }
            RemoteErrorKind::Network => State::Unavailable {
                reason: Reason::Network,
            },
            _ => State::Unavailable {
                reason: Reason::InvalidResponse,
            },
        },
        Err(_) => State::Unavailable {
            reason: Reason::InvalidResponse,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_failures_preserve_their_typed_action_without_server_details() {
        for (kind, reason) in [
            (RemoteErrorKind::Incompatible, Reason::IncompatibleVersion),
            (
                RemoteErrorKind::UnsupportedCapabilities,
                Reason::UnsupportedCapabilities,
            ),
        ] {
            let state = classify(Err(SttError::Remote(fono_stt::remote::RemoteError {
                kind,
                status: Some(404),
                retry_after_seconds: None,
            })));
            assert_eq!(state, State::Unavailable { reason });
            assert!(!serde_json::to_string(&state).unwrap().contains("404"));
        }
    }

    #[test]
    fn discovered_missing_or_failed_model_is_not_a_malformed_server_response() {
        for kind in [
            RemoteErrorKind::ModelNotFound,
            RemoteErrorKind::ModelNotReady,
            RemoteErrorKind::WorkerFailed,
        ] {
            assert_eq!(
                classify(Err(SttError::Remote(fono_stt::remote::RemoteError {
                    kind,
                    status: Some(404),
                    retry_after_seconds: None,
                }))),
                State::ModelNotReady,
            );
        }
    }
    #[test]
    fn server_and_model_states_are_hints_without_inventing_a_local_fallback() {
        let mut health = RemoteHealth {
            server: RemoteServerHealth::Ready,
            selected_model: RemoteModelHealth::Unknown,
            interactive_queue: 0,
            batch_queue: 0,
        };
        assert_eq!(classify(Ok(health)), State::Ready);
        health.selected_model = RemoteModelHealth::Busy;
        assert_eq!(classify(Ok(health)), State::Busy);
        health.selected_model = RemoteModelHealth::Unloaded;
        assert_eq!(classify(Ok(health)), State::ModelNotReady);
        health.server = RemoteServerHealth::Starting;
        assert_eq!(classify(Ok(health)), State::Starting);
        health.server = RemoteServerHealth::Stopping;
        assert_eq!(classify(Ok(health)), State::Stopping);
    }
    #[test]
    fn cancel_and_untrusted_errors_produce_only_typed_state() {
        assert_eq!(
            classify(Err(SttError::Cancelled("PRIVATE".into()))),
            State::Cancelled
        );
        let encoded = serde_json::to_string(&classify(Err(SttError::Config(
            "PRIVATE token/path".into(),
        ))))
        .unwrap();
        assert!(!encoded.contains("PRIVATE"));
        assert!(encoded.contains("invalid_response"));
    }
}
