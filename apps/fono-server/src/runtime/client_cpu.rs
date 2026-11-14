//! Opt-in real inference acceptance: the native client crosses a production
//! HTTP listener and ServerJobs/ModelPool into CPU Whisper. Missing public
//! resources fail explicitly; this module never downloads or records audio.

use super::*;
use fono_service::audio_ingest::decode_file;
use fono_stt::remote::{
    RemoteEndpoint, RemoteModelHealth, RemoteRecognizer, RemoteServerHealth, RemoteTimeouts,
    RemoteTranscriptionClient,
};
use fono_types::{AccelerationMode, RemoteBackend, Transcript, WhisperArtifact};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

const FIXTURES: &str = include_str!("../../tests/fixtures/remote-client-cpu.json");
const WORK_TIMEOUT: Duration = Duration::from_secs(180);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
struct Resource {
    basename: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct AudioResource {
    #[serde(flatten)]
    file: Resource,
    sample_rate: u32,
    channels: u16,
    samples: usize,
    reference_words: Vec<String>,
}

#[derive(Deserialize)]
struct Fixtures {
    schema: u32,
    model: Resource,
    audio: AudioResource,
}

#[derive(Serialize)]
struct Receipt {
    schema: u32,
    status: &'static str,
    model: &'static str,
    backend: &'static str,
    model_sha256: String,
    audio_sha256: String,
    audio_samples: usize,
    reference_words_matched: usize,
    request_ms: u64,
    server_total_ms: u64,
    batch_elapsed_ms: u64,
    batch_segments: usize,
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        // Reuse the production helper's private, randomly named directory and
        // its guarded cleanup; only this test's credentials/uploads live here.
        Self(create_upload_dir().expect("cannot allocate isolated CPU E2E directory"))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = cleanup_upload_dir(&self.0);
    }
}

fn verified_resource(variable: &str, resource: &Resource) -> PathBuf {
    let requested = std::env::var_os(variable).unwrap_or_else(|| {
        panic!("{variable} is required; prepare the pinned public CPU fixtures")
    });
    let path = fs::canonicalize(requested)
        .unwrap_or_else(|_| panic!("{variable} must identify an existing public fixture"));
    let metadata = fs::metadata(&path).expect("cannot inspect public CPU fixture");
    assert!(metadata.is_file(), "public CPU fixture must be a file");
    assert_eq!(
        metadata.len(),
        resource.bytes,
        "public fixture size mismatch"
    );
    let mut file = fs::File::open(&path).expect("cannot read public CPU fixture");
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .expect("cannot hash public CPU fixture");
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        resource.sha256,
        "public fixture SHA-256 mismatch"
    );
    path
}

fn configuration(directory: &Directory, model: &Path) -> ServerConfig {
    use crate::config::{
        LoggingSettings, MetricsSettings, ModelPlacement, ModelResidency, ModelSettings,
        QueueSettings, RateLimitSettings, ServerSettings,
    };
    ServerConfig {
        server: ServerSettings {
            bind: "127.0.0.1:0".parse().unwrap(),
            client_url: None,
            allowed_hosts: vec!["127.0.0.1".into()],
            trusted_private_network: false,
            tokens_file: directory.0.join("tokens"),
            max_upload_mb: 1,
            max_audio_seconds: 15,
            body_read_timeout: Duration::from_secs(10),
            shutdown_grace: CLEANUP_TIMEOUT,
        },
        tls: None,
        models: ModelSettings {
            device: "cpu".into(),
            // The fixture remains in its verified cache and is read in place.
            dir: model.parent().unwrap().to_owned(),
            default: "whisper:tiny".into(),
            preload: vec!["whisper:tiny".into()],
            max_loaded: 1,
            client_model_load: false,
            placement: vec![ModelPlacement {
                model: "whisper:tiny".into(),
                device: "cpu".into(),
                residency: ModelResidency::Resident,
                idle_unload_after: None,
            }],
        },
        queue: QueueSettings {
            max_queued: 2,
            max_inflight_per_token: 2,
            cpu_parallel: 1,
        },
        metrics: MetricsSettings { enabled: false },
        logging: LoggingSettings {
            level: LogLevel::Off,
        },
        rate_limit: RateLimitSettings {
            enabled: true,
            // File-job polling is deliberately unrestricted in this inference
            // gate; rate-limit behavior has separate deterministic coverage.
            requests_per_minute: 10_000,
            burst: 1_000,
        },
    }
}

fn matches_public_reference(text: &str, audio: &AudioResource) -> bool {
    let words: HashSet<String> = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    audio
        .reference_words
        .iter()
        .all(|word| words.contains(word))
}

fn validate_transcript(
    transcript: &Transcript,
    artifact: &WhisperArtifact,
    audio: &AudioResource,
) -> Result<(), &'static str> {
    let remote = transcript
        .remote
        .as_ref()
        .ok_or("real recognition omitted remote metadata")?;
    if transcript.device.as_deref() != Some("remote:cpu")
        || remote.backend != RemoteBackend::Cpu
        || remote.device != "cpu"
        || remote.model != artifact.id()
        || !remote.valid()
    {
        return Err("real recognition returned unexpected CPU/model metadata");
    }
    if !matches_public_reference(&transcript.text, audio) {
        // Never include a transcript in either an assertion or the receipt.
        return Err("real CPU recognition missed the public speech reference");
    }
    let expected_audio = audio.samples as f32 / audio.sample_rate as f32;
    if !transcript
        .audio_secs
        .is_some_and(|seconds| (seconds - expected_audio).abs() < 0.001)
        || remote.server_total_ms == 0
        || remote.request_ms == 0
        || remote.network_ms.is_some()
    {
        return Err("real recognition returned invalid audio/request timings");
    }
    Ok(())
}

fn exercise(
    endpoint: RemoteEndpoint,
    artifact: WhisperArtifact,
    samples: Vec<i16>,
    fixtures: Fixtures,
    cancellation: OperationCancellation,
) -> Result<Receipt, &'static str> {
    let activity = Arc::new(fono_stt::NoopActivity);
    let client = RemoteRecognizer::connect(
        endpoint.clone(),
        artifact.id(),
        activity.clone(),
        &cancellation,
    )
    .map_err(|_| "native client discovery against the real CPU server failed")?;
    let health = client
        .probe_health(&cancellation)
        .map_err(|_| "native client CPU health check failed")?;
    if health.server != RemoteServerHealth::Ready
        || health.selected_model != RemoteModelHealth::Ready
    {
        return Err("production readiness did not report the loaded CPU model");
    }
    let transcript = client
        .transcribe(
            &samples,
            fono_stt::RecognitionOptions {
                language: "en",
                decoding: Default::default(),
                glossary: &[],
            },
            &cancellation,
        )
        .map_err(|_| "native client real CPU transcription failed")?;
    validate_transcript(&transcript, &artifact, &fixtures.audio)?;
    let remote = transcript.remote.as_ref().unwrap();

    // A second actual decode travels through the file-job queue and native
    // multipart uploader/poller; sharing weights does not substitute a result.
    let batch =
        RemoteTranscriptionClient::connect(endpoint, artifact.id(), activity, &cancellation)
            .map_err(|_| "native file client CPU discovery failed")?;
    let started = std::time::Instant::now();
    let batch_result = batch
        .transcribe_pcm_job(&samples, "en", &cancellation, &|_| {})
        .map_err(|_| "native file client real CPU transcription failed")?;
    if batch_result.model != "whisper:tiny"
        || batch_result.backend.as_deref() != Some("remote:cpu")
        || !matches_public_reference(&batch_result.text, &fixtures.audio)
        || batch_result.segments.is_empty()
    {
        return Err("real CPU queued result lacks expected model/backend/speech/timestamps");
    }
    Ok(Receipt {
        schema: 1,
        status: "passed",
        model: "whisper:tiny",
        backend: "cpu",
        model_sha256: fixtures.model.sha256,
        audio_sha256: fixtures.audio.file.sha256,
        audio_samples: samples.len(),
        reference_words_matched: fixtures.audio.reference_words.len(),
        request_ms: remote.request_ms,
        server_total_ms: remote.server_total_ms,
        batch_elapsed_ms: started.elapsed().as_millis() as u64,
        batch_segments: batch_result.segments.len(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires SHA-256 pinned public tiny/JFK fixtures; run the explicit CPU E2E gate"]
async fn real_cpu_remote_client_roundtrip() {
    let fixtures: Fixtures = serde_json::from_str(FIXTURES).expect("invalid CPU fixture manifest");
    assert_eq!(fixtures.schema, 1);
    let model = verified_resource("FONO_TST02_MODEL", &fixtures.model);
    let wav = verified_resource("FONO_TST02_WAV", &fixtures.audio.file);
    assert_eq!(model.file_name().unwrap(), fixtures.model.basename.as_str());
    let artifact = WhisperArtifact::all()
        .unwrap()
        .into_iter()
        .find(|artifact| artifact.variant == "tiny")
        .expect("tiny must be in the managed model catalog");
    assert_eq!(artifact.bytes, fixtures.model.bytes);
    assert_eq!(artifact.sha256, fixtures.model.sha256);
    let policy = AudioIngestPolicy {
        max_file_bytes: 1024 * 1024,
        max_duration_seconds: 15,
    };
    let audio = decode_file(&wav, policy).expect("cannot decode the pinned public speech WAV");
    assert_eq!(audio.source_sample_rate, fixtures.audio.sample_rate);
    assert_eq!(audio.source_channels, fixtures.audio.channels);
    assert_eq!(audio.pcm_samples.len(), fixtures.audio.samples);
    let directory = Directory::new();
    let config = configuration(&directory, &model);
    let secret =
        crate::tokens::create(&config.server.tokens_file, "cpu-e2e", TokenScope::Recognize)
            .expect("cannot create the isolated CPU E2E credential");
    let verifier: Arc<dyn TokenVerifier> = Arc::new(
        FileTokenVerifier::open(config.server.tokens_file.clone())
            .expect("cannot open the isolated token verifier"),
    );
    let plan = required_preload(&config).unwrap().remove(0);
    let activity = ServerActivity::new();
    let pool = Arc::new(
        ModelPool::new(
            SttEnvironment {
                models_dir: config.models.dir.clone(),
                activity: activity.clone(),
                ..SttEnvironment::default()
            },
            1,
        )
        .expect("cannot construct the real CPU model pool"),
    );
    let preloaded = tokio::task::spawn_blocking({
        let pool = Arc::clone(&pool);
        let selection = plan.selection.clone();
        move || {
            pool.acquire_on_device_verified(
                &selection,
                AccelerationMode::Cpu,
                None,
                fono_stt::ModelResidency::Resident,
                0,
                &OperationCancellation::default(),
                || Ok(()),
            )
            .map(drop)
        }
    })
    .await
    .expect("CPU preload task panicked");
    if preloaded.is_err() {
        pool.begin_shutdown();
        let _ = drain_pool(&pool, Instant::now() + CLEANUP_TIMEOUT).await;
        panic!("the verified tiny fixture could not load on the real CPU backend");
    }
    let jobs = Arc::new(
        ServerJobs::new(Arc::clone(&pool), plan.clone(), &config, activity.clone())
            .expect("cannot start actual CPU server queue workers"),
    );
    let health = Arc::new(HealthState {
        verifier: Arc::clone(&verifier),
        pool: Arc::clone(&pool),
        jobs: jobs.jobs(),
        queued_interactive: {
            let jobs = Arc::clone(&jobs);
            Arc::new(move || jobs.queued_interactive())
        },
        default_id: plan.selection.id.clone(),
        default_name: plan.id.clone(),
        models: vec![plan],
        loading: Mutex::new(HashSet::new()),
        failed: Mutex::new(HashSet::new()),
        stopping: AtomicBool::new(false),
    });
    let api = ApiState::new(
        config.clone(),
        Arc::clone(&pool),
        jobs.jobs(),
        Arc::clone(&verifier),
        activity.clone(),
    )
    .expect("cannot construct production CPU API state");
    let recognize = RecognizeState::new(
        jobs.clone(),
        activity.clone(),
        directory.0.clone(),
        policy,
        config.server.body_read_timeout,
    );
    let rest = RestApiState::new("", SERVER_API_VERSION, activity.clone())
        .with_token_verifier(verifier)
        .with_jobs(jobs.jobs())
        .with_upload_dir(directory.0.clone())
        .with_upload_policy(policy)
        .with_body_read_timeout(config.server.body_read_timeout);
    let app = server_router(
        rest,
        Arc::clone(&health),
        recognize,
        policy.max_file_bytes as usize,
        config.server.allowed_hosts,
        api,
    );
    let listener = tokio::net::TcpListener::bind(config.server.bind)
        .await
        .expect("cannot bind the test-owned ephemeral loopback listener");
    let endpoint = RemoteEndpoint::new(
        &format!("http://{}/", listener.local_addr().unwrap()),
        &secret,
    )
    .unwrap()
    .with_timeouts(RemoteTimeouts {
        connect: Duration::from_secs(5),
        response_base: Duration::from_secs(120),
        audio_multiplier: 1.0,
    })
    .unwrap();
    let (stop, stopped) = oneshot::channel();
    let mut host = tokio::spawn(async move {
        rest_api::serve_with_shutdown(app, listener, async move {
            let _ = stopped.await;
        })
        .await
    });
    let cancellation = OperationCancellation::default();
    let mut work = tokio::task::spawn_blocking({
        let cancellation = cancellation.clone();
        move || {
            exercise(
                endpoint,
                artifact,
                audio.pcm_samples,
                fixtures,
                cancellation,
            )
        }
    });
    let (result, client_completed) = match tokio::time::timeout(WORK_TIMEOUT, &mut work).await {
        Ok(Ok(result)) => (result, true),
        Ok(Err(_)) => (Err("real CPU client task panicked"), true),
        Err(_) => {
            cancellation.cancel();
            (
                Err("real CPU client exceeded its acceptance deadline"),
                false,
            )
        }
    };

    // Cleanup precedes every result assertion, including client failures.
    cancellation.cancel();
    health.stopping.store(true, Ordering::Release);
    activity.close();
    jobs.cancel_all();
    jobs.shutdown_worker();
    let _ = stop.send(());
    let cleanup_deadline = Instant::now() + CLEANUP_TIMEOUT;
    let joined = tokio::time::timeout_at(
        cleanup_deadline,
        tokio::task::spawn_blocking({
            let jobs = Arc::clone(&jobs);
            move || jobs.join_worker().is_ok()
        }),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or(false);
    pool.begin_shutdown();
    let drained = drain_pool(&pool, cleanup_deadline).await;
    let served = tokio::time::timeout_at(cleanup_deadline, &mut host)
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|result| result.is_ok());
    if !served {
        host.abort();
    }
    let client_joined = client_completed
        || tokio::time::timeout_at(cleanup_deadline, &mut work)
            .await
            .is_ok();
    assert!(
        joined && drained && served && client_joined,
        "CPU E2E cleanup did not finish"
    );
    assert_eq!(activity.active(), 0, "CPU E2E leaked an activity lease");
    let receipt = result.unwrap_or_else(|message| panic!("{message}"));
    if let Some(report) = std::env::var_os("FONO_TST02_REPORT") {
        let report = PathBuf::from(report);
        if let Some(parent) = report
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .expect("cannot create the requested CPU E2E receipt directory");
        }
        fs::write(report, serde_json::to_vec_pretty(&receipt).unwrap())
            .expect("cannot write the requested numerical CPU E2E receipt");
    }
    println!("{}", serde_json::to_string(&receipt).unwrap());
}
