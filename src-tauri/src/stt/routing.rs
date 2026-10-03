use super::*;

pub(super) enum EngineState {
    Empty,
    Embedded {
        engine: Arc<Mutex<EmbeddedEngine>>,
        device: &'static str,
    },
    Worker {
        session: Arc<WorkerMailbox>,
        device: &'static str,
    },
}

/// Observable readiness of the selected STT backend. It is separate from the
/// routing state so callers can inspect loading/failure without taking a
/// long-lived engine lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SttReadiness {
    Unloaded,
    Loading,
    Ready { device: String },
    Failed { message: String },
}

/// Point-in-time health probe that never waits for an active transcription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SttHealth {
    Unloaded,
    Busy { device: String },
    Ready { device: String },
    Failed { device: String, message: String },
}

pub(super) type SttReadinessObserver = Arc<dyn Fn(SttReadiness) + Send + Sync>;

#[derive(Clone)]
pub(super) enum ActiveEngine {
    Embedded(Arc<Mutex<EmbeddedEngine>>),
    Worker(Arc<WorkerMailbox>),
}

impl EngineState {
    pub(super) fn compatible(&mut self, model_path: &str, candidate: &EngineCandidate) -> bool {
        match (self, candidate) {
            (Self::Embedded { engine, .. }, EngineCandidate::Embedded { use_gpu }) => {
                let current = engine.lock();
                current.model_path == model_path && current.use_gpu == *use_gpu
            }
            (Self::Worker { session, .. }, EngineCandidate::Worker { backend, path }) => {
                session.compatible_with(model_path, *backend, path)
            }
            _ => false,
        }
    }

    pub(super) fn device(&self) -> String {
        match self {
            Self::Empty => "CPU".into(),
            Self::Embedded { device, .. } | Self::Worker { device, .. } => (*device).into(),
        }
    }

    pub(super) fn active(&self) -> Option<ActiveEngine> {
        match self {
            Self::Empty => None,
            Self::Embedded { engine, .. } => Some(ActiveEngine::Embedded(Arc::clone(engine))),
            Self::Worker { session, .. } => Some(ActiveEngine::Worker(Arc::clone(session))),
        }
    }

    pub(super) fn contains(&self, active: &ActiveEngine) -> bool {
        match (self, active) {
            (Self::Embedded { engine, .. }, ActiveEngine::Embedded(active)) => {
                Arc::ptr_eq(engine, active)
            }
            (Self::Worker { session, .. }, ActiveEngine::Worker(active)) => {
                Arc::ptr_eq(session, active)
            }
            _ => false,
        }
    }
}

impl ActiveEngine {
    pub(super) fn transcribe(&self, samples: &[i16], language: &str) -> AppResult<Transcript> {
        match self {
            Self::Embedded(engine) => {
                engine
                    .lock()
                    .transcribe(samples, language, &OperationCancellation::default())
            }
            Self::Worker(session) => session.transcribe(samples, language),
        }
    }

    pub(super) fn transcribe_cancellable(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled(
                "STT request cancelled before start".into(),
            ));
        }
        let result = match self {
            Self::Embedded(engine) => engine.lock().transcribe(samples, language, cancellation),
            Self::Worker(session) => {
                session.transcribe_cancellable(samples, language, cancellation)
            }
        };
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled("STT request cancelled".into()));
        }
        result
    }

    pub(super) fn is_worker(&self) -> bool {
        matches!(self, Self::Worker(_))
    }

    pub(super) fn health(&self, device: String) -> SttHealth {
        match self {
            Self::Embedded(engine) => {
                if engine.try_lock().is_some() {
                    SttHealth::Ready { device }
                } else {
                    SttHealth::Busy { device }
                }
            }
            Self::Worker(session) => match session.health() {
                MailboxHealth::Busy => SttHealth::Busy { device },
                MailboxHealth::Ready => SttHealth::Ready { device },
                MailboxHealth::Failed(error) => SttHealth::Failed {
                    device,
                    message: error.to_string(),
                },
            },
        }
    }
}
