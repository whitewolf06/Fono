//! Cross-crate contract tests: real desktop HTTP clients, listener, server
//! router, middleware, audio parser and queue. Only inference ports are fake.

use super::*;
use crate::recognize::InteractiveRecognizer;
use fono_service::rest_api::TokenPrincipal;
use fono_service::transcription_jobs::{
    ClientInflightLimit, NoCaptureGate, NoInteractiveActivity, TranscriptionJobQueue,
    TranscriptionJobWorker,
};
use fono_service::transcription_service::{TranscriptionRuntime, TranscriptionService};
use fono_stt::remote::RemoteEndpoint;
use fono_types::{Transcript, TranscriptionServiceError, WhisperModelSize};

mod interactive;
mod lifecycle;

pub(super) const TOKEN: &str = "tst02-recognize-token";
pub(super) const MODEL: &str = "whisper:tiny";
pub(super) const CLIENT_KEY: &str = "tst02-client";

pub(super) fn model() -> ModelId {
    ModelId::whisper(WhisperModelSize::Tiny)
}

pub(super) fn options() -> fono_stt::RecognitionOptions<'static> {
    fono_stt::RecognitionOptions {
        language: "en",
        decoding: Default::default(),
        glossary: &[],
    }
}

pub(super) struct Tokens {
    pub(super) active: AtomicBool,
}

impl TokenVerifier for Tokens {
    fn verify(&self, token: &str) -> bool {
        token == TOKEN && self.active.load(Ordering::Acquire)
    }

    fn scope(&self, token: &str) -> Option<TokenScope> {
        self.verify(token).then_some(TokenScope::Recognize)
    }

    fn principal(&self, token: &str) -> Option<TokenPrincipal> {
        self.scope(token).map(|scope| TokenPrincipal {
            client_key: CLIENT_KEY.into(),
            scope,
        })
    }
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("fono-tst02-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        // Only this test's direct child of the system temp directory is owned.
        let Ok(temp) = std::env::temp_dir().canonicalize() else {
            return;
        };
        if self.0.parent() == Some(temp.as_path())
            && self
                .0
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("fono-tst02-"))
            && fs::symlink_metadata(&self.0).is_ok_and(|meta| !meta.file_type().is_symlink())
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

pub(super) struct Host {
    pub(super) url: String,
    pub(super) activity: Arc<ServerActivity>,
    pub(super) tokens: Arc<Tokens>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    worker: Arc<TranscriptionJobWorker>,
    _directory: Directory,
}

impl Host {
    pub(super) async fn start(recognizer: Arc<dyn InteractiveRecognizer>) -> Self {
        Self::start_with_runtime(recognizer, UnavailableBatch).await
    }

    pub(super) async fn start_with_runtime<R>(
        recognizer: Arc<dyn InteractiveRecognizer>,
        runtime: R,
    ) -> Self
    where
        R: TranscriptionRuntime + 'static,
    {
        let directory = Directory::new();
        let models_dir = directory.0.join("models");
        let upload_dir = directory.0.join("uploads");
        fs::create_dir(&models_dir).unwrap();
        fs::create_dir(&upload_dir).unwrap();
        let artifact = fono_types::WhisperArtifact::all()
            .unwrap()
            .into_iter()
            .find(|artifact| artifact.id() == model())
            .unwrap();
        // Discovery validates file metadata. No fake port opens these weights;
        // actual CPU inference has a separate pinned-model test module.
        fs::File::create(models_dir.join(artifact.filename))
            .unwrap()
            .set_len(artifact.bytes)
            .unwrap();
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/server.toml.example");
        let mut config = crate::config::load_and_validate_with_env(
            &path,
            std::iter::empty::<(std::ffi::OsString, std::ffi::OsString)>(),
        )
        .unwrap();
        config.models.dir = models_dir.clone();
        config.models.default = MODEL.into();
        config.models.preload = vec![MODEL.into()];
        config.models.placement.clear();
        config.models.max_loaded = 1;
        config.server.max_upload_mb = 1;
        config.server.max_audio_seconds = 1;
        config.rate_limit.burst = 1_000;
        config.rate_limit.requests_per_minute = 10_000;
        let plan = required_preload(&config).unwrap().remove(0);
        let activity = ServerActivity::new();
        let pool = Arc::new(
            ModelPool::new(
                SttEnvironment {
                    activity: activity.clone(),
                    models_dir,
                    ..SttEnvironment::default()
                },
                1,
            )
            .unwrap(),
        );
        let tokens = Arc::new(Tokens {
            active: AtomicBool::new(true),
        });
        let verifier: Arc<dyn TokenVerifier> = tokens.clone();
        let jobs: Arc<dyn TranscriptionJobs> = Arc::new(
            TranscriptionJobQueue::new(
                TranscriptionService::new(runtime),
                NoInteractiveActivity,
                4,
                activity.clone(),
                Arc::new(NoCaptureGate),
            )
            .with_client_limit(ClientInflightLimit::new(2)),
        );
        let worker = Arc::new(TranscriptionJobWorker::start(jobs.clone()).unwrap());
        let health = Arc::new(HealthState {
            verifier: verifier.clone(),
            pool: pool.clone(),
            jobs: jobs.clone(),
            queued_interactive: Arc::new(|| 0),
            default_id: plan.selection.id.clone(),
            default_name: plan.id.clone(),
            models: vec![plan],
            loading: Mutex::new(HashSet::new()),
            failed: Mutex::new(HashSet::new()),
            stopping: AtomicBool::new(false),
        });
        let policy = AudioIngestPolicy {
            max_file_bytes: 1024 * 1024,
            max_duration_seconds: 1,
        };
        let recognize = RecognizeState::new(
            recognizer,
            activity.clone(),
            upload_dir.clone(),
            policy,
            Duration::from_secs(5),
        );
        let api = ApiState::new(
            config.clone(),
            pool,
            jobs.clone(),
            verifier.clone(),
            activity.clone(),
        )
        .unwrap();
        let rest = RestApiState::new("", SERVER_API_VERSION, activity.clone())
            .with_token_verifier(verifier)
            .with_jobs(jobs)
            .with_upload_dir(upload_dir)
            .with_upload_policy(policy)
            .with_body_read_timeout(Duration::from_secs(5));
        let router = server_router(
            rest,
            health,
            recognize,
            policy.max_file_bytes as usize,
            config.server.allowed_hosts,
            api,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(rest_api::serve_with_shutdown(
            router,
            listener,
            async move {
                let _ = stopped.await;
            },
        ));
        Self {
            url,
            activity,
            tokens,
            stop: Some(stop),
            task: Some(task),
            worker,
            _directory: directory,
        }
    }

    pub(super) fn endpoint(&self, token: &str) -> RemoteEndpoint {
        RemoteEndpoint::new(&self.url, token).unwrap()
    }

    pub(super) async fn stop(mut self) {
        self.worker.shutdown();
        let worker = self.worker.clone();
        tokio::task::spawn_blocking(move || worker.join())
            .await
            .unwrap()
            .unwrap();
        self.activity.close();
        assert_eq!(
            self.activity.active(),
            0,
            "actual server work must be drained"
        );
        let _ = self.stop.take().unwrap().send(());
        let mut task = self.task.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                task.abort();
                panic!("HTTP fixture did not stop after its work drained");
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.worker.shutdown();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Panic cleanup only. Cancellation/disconnect evidence never relies on
        // aborting the server; the tests own their request or forwarding socket.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

struct UnavailableBatch;

impl TranscriptionRuntime for UnavailableBatch {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        Ok(MODEL.into())
    }

    fn transcribe(
        &self,
        _samples: &[i16],
        _language: &str,
        _cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        Err(TranscriptionServiceError::ModelNotReady(
            "test batch inference is not configured".into(),
        ))
    }
}
