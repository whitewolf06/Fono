use super::*;

pub(super) enum EngineState {
    Empty,
    Embedded {
        engine: Arc<EmbeddedRecognizer>,
        device: &'static str,
        model_path: String,
        use_gpu: bool,
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
    Embedded(Arc<EmbeddedRecognizer>),
    Worker(Arc<WorkerMailbox>),
}

impl EngineState {
    pub(super) fn model(&self) -> Option<&ModelId> {
        use super::recognizer::Recognizer;
        match self {
            Self::Empty => None,
            Self::Embedded { engine, .. } => Some(engine.model()),
            Self::Worker { session, .. } => Some(session.model()),
        }
    }
    pub(super) fn compatible(&self, model_path: &str, candidate: &EngineCandidate) -> bool {
        match (self, candidate) {
            (
                Self::Embedded {
                    model_path: current,
                    use_gpu: active_gpu,
                    ..
                },
                EngineCandidate::Embedded { use_gpu },
            ) => current == model_path && active_gpu == use_gpu,
            (Self::Worker { session, .. }, EngineCandidate::Worker { backend, path, .. }) => {
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
            Self::Worker { session, .. } => session
                .has_resources()
                .then(|| ActiveEngine::Worker(Arc::clone(session))),
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
    pub(super) fn recognizer(&self) -> &dyn super::recognizer::Recognizer {
        match self {
            Self::Embedded(engine) => engine.as_ref(),
            Self::Worker(session) => session.as_ref(),
        }
    }

    pub(super) fn worker_faulted(&self) -> bool {
        matches!(self, Self::Worker(session) if session.is_faulted())
    }

    pub(super) fn health(&self, device: String) -> SttHealth {
        match self {
            Self::Embedded(engine) => {
                if engine.is_idle() {
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
                    message: error.user_message(),
                },
            },
        }
    }
}
