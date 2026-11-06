use super::*;
use crate::recognition_options::{RecognitionOptions, RecognitionOutput, TimestampMode};
use crate::recognize::AdmittedRecognition;
use fono_stt::error::SttError;
use fono_stt::remote::{RemoteError, RemoteErrorKind, RemoteRecognizer, RemoteTranscriptionClient};
use fono_stt::scheduler::QueueClass;
use fono_stt_protocol::{BackendKind, DecodingStrategy};
use fono_types::{RemoteBackend, SubtitleSegment, TranscriptionRequest, TranscriptionResult};
use std::sync::atomic::AtomicUsize;

#[derive(Debug)]
struct Captured {
    client: String,
    request: TranscriptionRequest,
    class: QueueClass,
    options: RecognitionOptions,
}

#[derive(Default)]
struct FixtureRecognizer {
    admissions: AtomicUsize,
    captured: Mutex<Vec<Captured>>,
    admission_error: Mutex<Option<TranscriptionServiceError>>,
    worker_error: Mutex<Option<TranscriptionServiceError>>,
    backend: Mutex<Option<(&'static str, &'static str)>>,
}

impl InteractiveRecognizer for FixtureRecognizer {
    fn admit(
        self: Arc<Self>,
        client_key: &str,
    ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
        self.admissions.fetch_add(1, Ordering::AcqRel);
        if let Some(error) = self.admission_error.lock().unwrap().take() {
            return Err(error);
        }
        let client = client_key.to_owned();
        Ok(Box::new(move |request, class, options, _cancellation| {
            let (backend, device) = self.backend.lock().unwrap().unwrap_or(("CPU", "cpu"));
            let mut result = TranscriptionResult::from_transcript(
                transcript(&request.pcm_samples, &request.language, backend),
                request.model.clone().unwrap(),
            );
            if options.timestamps == TimestampMode::Segments {
                result.segments = segments();
            }
            self.captured.lock().unwrap().push(Captured {
                client,
                request,
                class,
                options,
            });
            if let Some(error) = self.worker_error.lock().unwrap().take() {
                return Err(error);
            }
            Ok(RecognitionOutput {
                result,
                words: Vec::new(),
                device: Some(device.into()),
            })
        }))
    }
}

fn transcript(samples: &[i16], language: &str, backend: &str) -> Transcript {
    Transcript {
        text: "synthetic remote fixture".into(),
        detected_language: Some(language.into()),
        transcribe_secs: Some(0.001),
        audio_secs: Some(samples.len() as f32 / 16_000.0),
        device: Some(backend.into()),
        remote: None,
    }
}

fn segments() -> Vec<SubtitleSegment> {
    vec![SubtitleSegment {
        text: "synthetic remote fixture".into(),
        start_ms: 10,
        end_ms: 50,
    }]
}

fn pcm() -> Vec<i16> {
    [0x1234, -2, i16::MIN, i16::MAX, 0x0102, -0x1234, 0, 1]
        .into_iter()
        .cycle()
        .take(1_600)
        .collect()
}

fn batch_pcm() -> Vec<i16> {
    // A low-amplitude ramp survives the file decoder's float normalization
    // exactly, so this assertion concerns upload ordering and signed samples.
    (-800..800).collect()
}

async fn connect(host: &Host, token: &str) -> Result<RemoteRecognizer, SttError> {
    let endpoint = host.endpoint(token);
    tokio::task::spawn_blocking(move || {
        RemoteRecognizer::connect(
            endpoint,
            model(),
            Arc::new(fono_stt::NoopActivity),
            &OperationCancellation::default(),
        )
    })
    .await
    .unwrap()
}

async fn transcribe(client: &RemoteRecognizer) -> Result<Transcript, SttError> {
    let client = client.clone();
    tokio::task::spawn_blocking(move || {
        client.transcribe(&pcm(), options(), &OperationCancellation::default())
    })
    .await
    .unwrap()
}

fn remote_error(error: SttError, kind: RemoteErrorKind, status: Option<u16>) -> RemoteError {
    let SttError::Remote(error) = error else {
        panic!("expected a classified remote failure");
    };
    assert_eq!(error.kind, kind);
    assert_eq!(error.status, status);
    assert!(!format!("{error:?} {error}").contains("PRIVATE"));
    assert!(!format!("{error:?} {error}").contains(TOKEN));
    error
}

#[tokio::test]
async fn pcm_options_and_actual_backend_cross_the_production_contract() {
    let recognizer = Arc::new(FixtureRecognizer::default());
    let host = Host::start(recognizer.clone()).await;
    let client = connect(&host, TOKEN).await.unwrap();
    let capabilities = client.capabilities();
    assert_eq!(client.model(), &model());
    assert_eq!(client.device_name(), "remote:cpu");
    assert_eq!(capabilities.max_segment_samples, 16_000);
    assert!(capabilities.window_transcription);
    assert!(capabilities.hotwords);
    assert!(capabilities.decoding_options);
    assert!(capabilities.automatic_language);
    assert!(!capabilities.initial_prompt);
    assert!(!capabilities.token_timestamps);

    for (backend, device, expected) in [
        ("CPU", "cpu", RemoteBackend::Cpu),
        ("CUDA", "cuda:2", RemoteBackend::Cuda),
        ("Vulkan", "vulkan", RemoteBackend::Vulkan),
    ] {
        *recognizer.backend.lock().unwrap() = Some((backend, device));
        let work = client.clone();
        let result = tokio::task::spawn_blocking(move || {
            work.transcribe(
                &pcm(),
                fono_stt::RecognitionOptions {
                    decoding: fono_stt_protocol::DecodingOptions {
                        strategy: DecodingStrategy::Accurate,
                        threads: None,
                    },
                    glossary: &["Fono".into(), "Kari server".into()],
                    ..options()
                },
                &OperationCancellation::default(),
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.text, "synthetic remote fixture");
        assert_eq!(result.detected_language.as_deref(), Some("en"));
        assert_eq!(result.audio_secs, Some(0.1));
        assert_eq!(result.device, Some(format!("remote:{device}")));
        // Discovery said CPU; history must describe this particular response.
        let remote = result.remote.unwrap();
        assert_eq!(remote.model, model());
        assert_eq!(remote.backend, expected);
        assert_eq!(remote.device, device);
        assert!(remote.valid());
        assert!(remote.request_ms < 30_000);
        assert!(remote.server_total_ms < 30_000);
        assert_eq!(remote.queue_ms, None);
        assert_eq!(remote.load_ms, None);
        assert_eq!(remote.decode_ms, None);
        assert_eq!(remote.network_ms, None);
        assert_eq!(result.transcribe_secs, None);
    }
    {
        let requests = recognizer.captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for captured in requests.iter() {
            assert_eq!(captured.client, CLIENT_KEY);
            assert_eq!(captured.request.pcm_samples, pcm());
            assert_eq!(captured.request.model.as_deref(), Some(MODEL));
            assert_eq!(captured.request.language, "en");
            assert_eq!(captured.class, QueueClass::Interactive);
            assert_eq!(
                captured.options.decoding.strategy,
                DecodingStrategy::Accurate
            );
            assert_eq!(captured.options.glossary, ["Fono", "Kari server"]);
            assert_eq!(captured.options.timestamps, TimestampMode::None);
            assert_eq!(captured.options.prompt, None);
        }
    }
    host.stop().await;
}

#[tokio::test]
async fn window_timestamps_are_rebased_from_the_real_server_response() {
    let recognizer = Arc::new(FixtureRecognizer::default());
    let host = Host::start(recognizer.clone()).await;
    let client = connect(&host, TOKEN).await.unwrap();
    let result = tokio::task::spawn_blocking(move || {
        client.transcribe_window(
            &pcm(),
            fono_stt::WindowOptions {
                recognition: options(),
                context: None,
                operation_id: 42,
                audio_start_sample: 32_000,
            },
            &OperationCancellation::default(),
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.backend, BackendKind::Cpu);
    assert_eq!(result.segments.len(), 1);
    assert_eq!(result.segments[0].start_sample, 32_160);
    assert_eq!(result.segments[0].end_sample, 32_800);
    assert_eq!(result.segments[0].text, "synthetic remote fixture");
    assert!(result.words.is_empty());
    assert_eq!(
        recognizer.captured.lock().unwrap()[0].options.timestamps,
        TimestampMode::Segments
    );
    host.stop().await;
}

#[tokio::test]
async fn unauthorized_and_revoked_credentials_never_admit_audio_and_can_recover() {
    let recognizer = Arc::new(FixtureRecognizer::default());
    let host = Host::start(recognizer.clone()).await;
    remote_error(
        connect(&host, "invalid-fixture-token").await.unwrap_err(),
        RemoteErrorKind::Unauthorized,
        Some(401),
    );
    let client = connect(&host, TOKEN).await.unwrap();
    assert_eq!(recognizer.admissions.load(Ordering::Acquire), 0);
    host.tokens.active.store(false, Ordering::Release);
    remote_error(
        transcribe(&client).await.unwrap_err(),
        RemoteErrorKind::Unauthorized,
        Some(401),
    );
    assert_eq!(recognizer.admissions.load(Ordering::Acquire), 0);
    assert!(recognizer.captured.lock().unwrap().is_empty());
    assert!(client.is_idle());
    host.tokens.active.store(true, Ordering::Release);
    assert_eq!(
        transcribe(&client).await.unwrap().text,
        "synthetic remote fixture"
    );
    assert_eq!(recognizer.admissions.load(Ordering::Acquire), 1);
    host.stop().await;
}

#[tokio::test]
async fn production_failures_are_classified_without_replay_and_allow_explicit_retry() {
    const PRIVATE: &str = "PRIVATE C:/user/profile model error token=fixture-secret";
    let recognizer = Arc::new(FixtureRecognizer::default());
    let host = Host::start(recognizer.clone()).await;
    let client = connect(&host, TOKEN).await.unwrap();
    let cases = [
        (
            TranscriptionServiceError::Busy(PRIVATE.into()),
            RemoteErrorKind::Busy,
            429,
            true,
        ),
        (
            TranscriptionServiceError::ModelNotFound(PRIVATE.into()),
            RemoteErrorKind::ModelNotFound,
            404,
            false,
        ),
        (
            TranscriptionServiceError::ModelNotReady(PRIVATE.into()),
            RemoteErrorKind::ModelNotReady,
            503,
            true,
        ),
        (
            TranscriptionServiceError::WorkerFailed(PRIVATE.into()),
            RemoteErrorKind::WorkerFailed,
            502,
            true,
        ),
        (
            TranscriptionServiceError::InvalidRequest(PRIVATE.into()),
            RemoteErrorKind::InvalidRequest,
            400,
            false,
        ),
        (
            TranscriptionServiceError::Failed(PRIVATE.into()),
            RemoteErrorKind::InvalidResponse,
            500,
            false,
        ),
    ];
    for (index, (failure, kind, status, retry_after)) in cases.into_iter().enumerate() {
        let admission_failure = matches!(failure, TranscriptionServiceError::Busy(_));
        if admission_failure {
            *recognizer.admission_error.lock().unwrap() = Some(failure);
        } else {
            *recognizer.worker_error.lock().unwrap() = Some(failure);
        }
        let before = recognizer.captured.lock().unwrap().len();
        let error = remote_error(transcribe(&client).await.unwrap_err(), kind, Some(status));
        assert_eq!(error.retry_after_seconds, retry_after.then_some(1));
        assert_eq!(recognizer.admissions.load(Ordering::Acquire), index * 2 + 1);
        assert_eq!(
            recognizer.captured.lock().unwrap().len(),
            before + usize::from(!admission_failure),
            "one requested attempt must never replay audio automatically"
        );
        assert!(client.is_idle());
        assert_eq!(
            transcribe(&client).await.unwrap().text,
            "synthetic remote fixture"
        );
        assert_eq!(recognizer.admissions.load(Ordering::Acquire), index * 2 + 2);
    }
    host.stop().await;
}

#[tokio::test]
async fn advertised_duration_and_language_are_enforced_before_audio_admission() {
    let recognizer = Arc::new(FixtureRecognizer::default());
    let host = Host::start(recognizer.clone()).await;
    let client = connect(&host, TOKEN).await.unwrap();
    let work = client.clone();
    let errors = tokio::task::spawn_blocking(move || {
        [
            work.transcribe(
                &vec![0; 16_001],
                options(),
                &OperationCancellation::default(),
            ),
            work.transcribe(
                &pcm(),
                fono_stt::RecognitionOptions {
                    language: "unsupported",
                    ..options()
                },
                &OperationCancellation::default(),
            ),
        ]
    })
    .await
    .unwrap();
    for error in errors {
        remote_error(error.unwrap_err(), RemoteErrorKind::InvalidRequest, None);
    }
    assert_eq!(recognizer.admissions.load(Ordering::Acquire), 0);
    assert!(recognizer.captured.lock().unwrap().is_empty());
    assert!(client.is_idle());
    host.stop().await;
}

type BatchCapture = Arc<Mutex<Vec<(Vec<i16>, String)>>>;

struct BatchRuntime {
    captured: BatchCapture,
}

impl TranscriptionRuntime for BatchRuntime {
    type Error = TranscriptionServiceError;

    fn configured_model(&self) -> Result<String, Self::Error> {
        Ok(MODEL.into())
    }

    fn transcribe(
        &self,
        samples: &[i16],
        language: &str,
        _cancellation: OperationCancellation,
    ) -> Result<Transcript, Self::Error> {
        self.captured
            .lock()
            .unwrap()
            .push((samples.to_vec(), language.into()));
        Ok(transcript(samples, language, "CPU"))
    }

    fn transcribe_with_progress(
        &self,
        samples: &[i16],
        language: &str,
        cancellation: OperationCancellation,
        progress: &(dyn Fn(f32) + Sync),
    ) -> Result<(Transcript, Vec<SubtitleSegment>), Self::Error> {
        progress(0.5);
        self.transcribe(samples, language, cancellation)
            .map(|result| (result, segments()))
    }
}

#[tokio::test]
async fn file_client_uploads_real_wav_to_the_real_queue_and_polls_its_result() {
    let interactive = Arc::new(FixtureRecognizer::default());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let host = Host::start_with_runtime(
        interactive.clone(),
        BatchRuntime {
            captured: captured.clone(),
        },
    )
    .await;
    let endpoint = host.endpoint(TOKEN);
    let (result, progress) = tokio::task::spawn_blocking(move || {
        let client = RemoteTranscriptionClient::connect(
            endpoint,
            model(),
            Arc::new(fono_stt::NoopActivity),
            &OperationCancellation::default(),
        )
        .unwrap();
        let progress = Mutex::new(Vec::new());
        let result = client.transcribe_pcm_job(
            &batch_pcm(),
            "en",
            &OperationCancellation::default(),
            &|fraction| progress.lock().unwrap().push(fraction),
        );
        (result, progress.into_inner().unwrap())
    })
    .await
    .unwrap();
    let result = result.unwrap();
    assert_eq!(result.text, "synthetic remote fixture");
    assert_eq!(result.model, MODEL);
    assert_eq!(result.backend.as_deref(), Some("remote:cpu"));
    assert_eq!(result.detected_language.as_deref(), Some("en"));
    assert_eq!(result.audio_seconds, Some(0.1));
    assert_eq!(result.segments, segments());
    assert_eq!(progress.last(), Some(&1.0));
    assert_eq!(*captured.lock().unwrap(), vec![(batch_pcm(), "en".into())]);
    assert_eq!(interactive.admissions.load(Ordering::Acquire), 0);
    host.stop().await;
}
