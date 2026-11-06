//! Authenticated, metadata-only preflight. It never loads a recognizer or
//! assumes that omission from the preload-only health list means no model.
use std::{sync::Arc, time::Duration};

use fono_core::OperationCancellation;
use fono_types::ModelId;
use serde::{Deserialize, Serialize};

use super::{
    compatibility, failure, transport::Transport, RemoteEndpoint, RemoteErrorKind, RemoteRecognizer,
};
use crate::{error::AppResult, ActivityGuard};

pub const REMOTE_HEALTH_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteServerHealth {
    Ready,
    Starting,
    Failed,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteModelHealth {
    Ready,
    Busy,
    Loading,
    Failed,
    Unloaded,
    Unknown,
}

/// Only known classifications and bounded counts leave the wire boundary.
/// No server address, model ID, device name or arbitrary error is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RemoteHealth {
    pub server: RemoteServerHealth,
    pub selected_model: RemoteModelHealth,
    pub interactive_queue: u32,
    pub batch_queue: u32,
}

#[derive(Deserialize)]
struct HealthResponse {
    status: RemoteServerHealth,
    models: Vec<ModelHealth>,
    queue: HealthQueue,
}
#[derive(Deserialize)]
struct ModelHealth {
    id: String,
    state: RemoteModelHealth,
}
#[derive(Deserialize)]
struct HealthQueue {
    interactive: u32,
    batch: u32,
}

/// A blocking host entry point for an otherwise unloaded client. Authentication
/// and TLS validation use exactly the same transport policy as recognition.
pub fn probe_health(
    endpoint: RemoteEndpoint,
    model: &ModelId,
    activity: Arc<dyn ActivityGuard>,
    cancellation: &OperationCancellation,
) -> AppResult<RemoteHealth> {
    let engine = model.engine;
    let model = public_model_id(model)?;
    let transport = Arc::new(Transport::new(endpoint, activity)?);
    probe(transport, model, engine, cancellation)
}

impl RemoteRecognizer {
    /// Reuses this recognizer's connection pool; it does not mutate its model
    /// capabilities, readiness, routing, or any ongoing transcription.
    pub fn probe_health(&self, cancellation: &OperationCancellation) -> AppResult<RemoteHealth> {
        probe(
            self.transport.clone(),
            self.wire_model.clone(),
            self.model.engine,
            cancellation,
        )
    }
}

fn public_model_id(model: &ModelId) -> AppResult<String> {
    if model.variant.is_empty()
        || model.variant.len() > 128
        || !model
            .variant
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return failure(RemoteErrorKind::InvalidRequest);
    }
    Ok(if model.engine == fono_types::SttEngine::Whisper {
        format!("whisper:{}", model.variant)
    } else {
        model.public_id()
    })
}

fn probe(
    http: Arc<Transport>,
    model: String,
    engine: fono_types::SttEngine,
    cancellation: &OperationCancellation,
) -> AppResult<RemoteHealth> {
    let task = http.clone();
    http.call(cancellation, REMOTE_HEALTH_TIMEOUT, || {
        Ok(async move {
            compatibility::discover(&task, &model, engine, compatibility::Route::Interactive)
                .await?;
            let (status, response) = task
                .health_json::<HealthResponse>(task.get("v1/health"), 64 * 1024)
                .await?;
            if (status == 200) != (response.status == RemoteServerHealth::Ready) {
                return failure(RemoteErrorKind::InvalidResponse);
            }
            let mut matches = response.models.into_iter().filter(|item| item.id == model);
            let selected_model = matches
                .next()
                .map_or(RemoteModelHealth::Unknown, |item| item.state);
            if matches.next().is_some() {
                return failure(RemoteErrorKind::InvalidResponse);
            }
            Ok(RemoteHealth {
                server: response.status,
                selected_model,
                interactive_queue: response.queue.interactive,
                batch_queue: response.queue.batch,
            })
        })
    })
}
