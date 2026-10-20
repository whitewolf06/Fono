use super::*;
use crate::pipeline::scheduler::SttScheduler;
use crate::stt::{SttEngine, SttReadiness, WorkerPaths};
use crate::types::{AccelerationMode, SttEngine as ModelEngine};
use fono_stt_protocol::{BackendKind, DecodingStrategy};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct OfflineFixture {
    model: ModelId,
    calls: AtomicUsize,
    cancel_on_return: bool,
    window: Mutex<Option<(u64, u64, String, String)>>,
}

impl OfflineFixture {
    fn new(cancel_on_return: bool) -> Self {
        Self {
            model: ModelId {
                engine: ModelEngine::Parakeet,
                variant: "contract-fixture".into(),
            },
            calls: AtomicUsize::new(0),
            cancel_on_return,
            window: Mutex::new(None),
        }
    }
}

impl Recognizer for OfflineFixture {
    fn load(_: &RecognizerLoad<'_>) -> AppResult<Self> {
        Err(AppError::Config("Test-only recognizer".into()))
    }
    fn model(&self) -> &ModelId {
        &self.model
    }
    fn device(&self) -> &'static str {
        "CPU"
    }
    fn capabilities(&self) -> RecognizerCapabilities {
        RecognizerCapabilities {
            max_segment_samples: 25 * 16_000,
            window_transcription: true,
            initial_prompt: false,
            hotwords: false,
            language_detection: false,
            automatic_language: false,
            token_timestamps: false,
            decoding_options: false,
            cancellation: CancellationSupport::BetweenSegments,
        }
    }
    fn transcribe(
        &self,
        _: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        assert!(
            options.glossary.is_empty(),
            "unsupported hotwords must not reach this backend"
        );
        self.calls.fetch_add(1, Ordering::AcqRel);
        if self.cancel_on_return {
            cancellation.cancel();
        }
        Ok(Transcript {
            text: "offline fixture".into(),
            detected_language: Some("ru".into()),
            transcribe_secs: Some(0.1),
            audio_secs: Some(1.0),
            device: Some(self.device().into()),
        })
    }
    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        *self.window.lock() = Some((
            options.operation_id,
            options.audio_start_sample,
            options.recognition.language.into(),
            options.context.unwrap_or_default().into(),
        ));
        let result = self.transcribe(samples, options.recognition, cancellation)?;
        Ok(WindowTranscript {
            text: result.text,
            segments: vec![],
            words: vec![],
            detected_language: result.detected_language,
            transcribe_secs: 0.1,
            audio_secs: 1.0,
            backend: BackendKind::Cpu,
        })
    }
}

fn options() -> RecognitionOptions<'static> {
    RecognitionOptions {
        language: "ru",
        glossary: &[],
        decoding: DecodingOptions::default(),
    }
}

#[test]
fn unsupported_backend_receives_neither_whisper_terms_nor_hotwords() {
    let fixture = OfflineFixture::new(false);
    let terms = ["PRIVATE TERM".into()];
    let opts = RecognitionOptions {
        glossary: &terms,
        ..options()
    };
    let cancellation = OperationCancellation::default();
    transcribe(&fixture, &[1; 160], opts, &cancellation).unwrap();
    transcribe_window(
        &fixture,
        &[1; 160],
        WindowOptions {
            recognition: opts,
            context: None,
            operation_id: 1,
            audio_start_sample: 0,
        },
        &cancellation,
    )
    .unwrap();
    assert_eq!(fixture.calls.load(Ordering::Acquire), 2);
}

#[test]
fn pre_cancelled_call_never_enters_a_recognizer() {
    let fixture = OfflineFixture::new(false);
    let cancellation = OperationCancellation::default();
    cancellation.cancel();
    assert!(matches!(
        transcribe(&fixture, &[0], options(), &cancellation),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(fixture.calls.load(Ordering::Acquire), 0);
}

#[test]
fn a_native_result_returning_after_cancellation_is_discarded() {
    let fixture = OfflineFixture::new(true);
    let cancellation = OperationCancellation::default();
    assert!(matches!(
        transcribe(&fixture, &[0], options(), &cancellation),
        Err(AppError::Cancelled(_))
    ));
    let cancellation = OperationCancellation::default();
    assert!(matches!(
        transcribe_window(
            &fixture,
            &[0],
            WindowOptions {
                recognition: options(),
                context: None,
                operation_id: 7,
                audio_start_sample: 16_000,
            },
            &cancellation
        ),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(fixture.calls.load(Ordering::Acquire), 2);
}

#[test]
fn unsupported_options_fail_before_native_work_and_leave_the_runtime_usable() {
    let fixture = OfflineFixture::new(false);
    let cancel = OperationCancellation::default();
    for options in [
        RecognitionOptions {
            language: "auto",
            ..options()
        },
        RecognitionOptions {
            decoding: DecodingOptions {
                strategy: DecodingStrategy::Accurate,
                threads: None,
            },
            ..options()
        },
        RecognitionOptions {
            decoding: DecodingOptions {
                threads: Some(0),
                ..Default::default()
            },
            ..options()
        },
    ] {
        assert!(matches!(
            transcribe(&fixture, &[0], options, &cancel),
            Err(AppError::Config(_))
        ));
    }
    assert!(matches!(
        transcribe_window(
            &fixture,
            &[0],
            WindowOptions {
                recognition: options(),
                context: Some("unsupported prompt"),
                operation_id: 9,
                audio_start_sample: 16_000,
            },
            &cancel
        ),
        Err(AppError::Config(_))
    ));
    assert_eq!(fixture.calls.load(Ordering::Acquire), 0);
    let result = transcribe(&fixture, &[0], options(), &cancel).unwrap();
    assert_eq!(result.text, "offline fixture");
    assert_eq!(result.device.as_deref(), Some("CPU"));
    assert_eq!(fixture.model().engine, ModelEngine::Parakeet);
}

#[test]
fn window_contract_preserves_operation_and_sample_offset_for_an_offline_backend() {
    let fixture = OfflineFixture::new(false);
    let result = transcribe_window(
        &fixture,
        &[0],
        WindowOptions {
            recognition: options(),
            context: None,
            operation_id: 42,
            audio_start_sample: 32_000,
        },
        &OperationCancellation::default(),
    )
    .unwrap();
    assert_eq!(
        *fixture.window.lock(),
        Some((42, 32_000, "ru".into(), String::new()))
    );
    assert_eq!(result.backend, BackendKind::Cpu);
}

#[test]
fn cancellation_does_not_release_a_permit_while_native_work_is_still_running() {
    let scheduler = Arc::new(SttScheduler::default());
    let cancel = OperationCancellation::default();
    let worker_cancel = cancel.clone();
    let worker_scheduler = scheduler.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let permit = worker_scheduler.acquire(false, &worker_cancel)?;
        run(&permit.cancellation, || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    cancel.cancel();
    assert!(scheduler.is_busy());
    assert!(crate::application::updates::activity::is_busy());
    release_tx.send(()).unwrap();
    assert!(matches!(
        worker.join().unwrap(),
        Err(AppError::Cancelled(_))
    ));
    assert!(!scheduler.is_busy());
}

#[test]
fn typed_loading_rejects_unavailable_engines_without_changing_readiness() {
    let engine = SttEngine::new();
    let model = SttModelSelection {
        id: ModelId {
            engine: ModelEngine::GigaAm,
            variant: "not-integrated".into(),
        },
        local_path: "not-a-whisper-model.onnx".into(),
    };
    assert!(matches!(
        engine.ensure_model_loaded(
            &model,
            AccelerationMode::Cpu,
            &WorkerPaths::default(),
            &OperationCancellation::default()
        ),
        Err(AppError::Config(_))
    ));
    assert_eq!(engine.readiness(), SttReadiness::Unloaded);
    assert_eq!(engine.model(), None);
    assert_eq!(engine.capabilities(), None);

    let cancel = OperationCancellation::default();
    cancel.cancel();
    let model = SttModelSelection::from_whisper_path("missing.bin".into());
    assert!(matches!(
        engine.ensure_model_loaded(
            &model,
            AccelerationMode::Cpu,
            &WorkerPaths::default(),
            &cancel
        ),
        Err(AppError::Cancelled(_))
    ));
    assert_eq!(engine.readiness(), SttReadiness::Unloaded);
}

#[test]
#[ignore = "requires the explicitly supplied pinned multilingual Tiny model"]
fn embedded_whisper_public_wav_contract() {
    use crate::application::audio_ingest::{decode_file, AudioIngestPolicy};
    use crate::stt::embedded::EmbeddedEngine;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    let model_path = PathBuf::from(
        std::env::var("FONO_RECOGNIZER_TEST_MODEL").expect("explicit model path required"),
    );
    let model_hash = format!("{:x}", Sha256::digest(std::fs::read(&model_path).unwrap()));
    assert_eq!(
        model_hash,
        "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stt-quality");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let cancel = OperationCancellation::default();
    let decoding = DecodingOptions {
        threads: Some(2),
        ..Default::default()
    };
    let mut direct = EmbeddedEngine::load(&model_path, false).unwrap();
    let routed = SttEngine::new();
    let model = SttModelSelection::from_whisper_path(model_path.to_string_lossy().into_owned());
    routed
        .ensure_model_loaded(
            &model,
            AccelerationMode::Cpu,
            &WorkerPaths::default(),
            &cancel,
        )
        .unwrap();
    assert_eq!(routed.model(), Some(model.id));
    assert_eq!(routed.device(), "CPU");
    assert!(routed.capabilities().unwrap().decoding_options);
    for id in ["en-01", "ru-01"] {
        let entry = manifest["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap();
        let path = root.join(entry["path"].as_str().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            entry["sha256"].as_str().unwrap()
        );
        let audio = decode_file(&path, AudioIngestPolicy::default()).unwrap();
        assert_eq!(audio.source_sample_rate, 16_000);
        assert_eq!(audio.source_channels, 1);
        let samples = &audio.pcm_samples;
        let language = entry["language"].as_str().unwrap();
        let expected = direct
            .transcribe(samples, language, &cancel, decoding, &[])
            .unwrap();
        let actual = routed
            .transcribe_cancellable_with_options(samples, language, cancel.clone(), decoding)
            .unwrap();
        assert!(!actual.text.is_empty());
        assert_eq!(actual.text, expected.text, "{id} ordinary text");
        assert_eq!(actual.detected_language, expected.detected_language);
        assert_eq!(actual.device, expected.device);
        assert_eq!(actual.audio_secs, expected.audio_secs);
        let context = Some("Fono");
        let expected = direct
            .transcribe_window(
                samples,
                context,
                &cancel,
                32_000,
                RecognitionOptions {
                    language,
                    decoding,
                    glossary: &[],
                },
            )
            .unwrap();
        let actual = routed
            .transcribe_window_with_options(
                samples, language, context, 42, &cancel, 32_000, decoding,
            )
            .unwrap();
        assert_eq!(actual.text, expected.text, "{id} window text");
        assert_eq!(actual.segments, expected.segments);
        assert_eq!(actual.words, expected.words);
        assert_eq!(actual.detected_language, expected.detected_language);
        assert_eq!(actual.backend, expected.backend);
        assert_eq!(actual.audio_secs, expected.audio_secs);
        eprintln!("{id}: direct and routed ordinary/window text, timestamps and device match");
    }
}
