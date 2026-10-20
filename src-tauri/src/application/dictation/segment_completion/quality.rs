//! Explicit FONO-124 replay: real recognizer, production capture/Stop, no microphone.
use super::*;
use crate::test_process as win32;
use crate::{
    audio::{AudioRecorder, RecordingWriter},
    pipeline::{pretranscription::START_AFTER_SAMPLES, Pipeline},
    stt::WorkerPaths,
    types::{AccelerationMode, SttModelSelection},
};
use fono_core::{OperationSource, TerminalReason};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Replay(Arc<Mutex<Option<RecordingWriter>>>);
impl AudioRecorder<RecordingWriter, AppError> for Replay {
    fn start(
        &self,
        _: Option<&str>,
        writer: RecordingWriter,
        _: Arc<AtomicBool>,
        _: Arc<AtomicU32>,
        _: usize,
    ) -> AppResult<()> {
        *self.0.lock() = Some(writer);
        Ok(())
    }
    fn stop(&self) -> AppResult<()> {
        Ok(())
    }
    fn shutdown(&self) {}
}

struct Memory {
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<Value>,
}
impl Memory {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = thread::spawn(move || {
            let before = win32::resources(std::process::id()).unwrap();
            let mut maximum = before.private_bytes;
            let mut samples = 1;
            while !stopped.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(100));
                maximum = maximum.max(win32::resources(std::process::id()).unwrap().private_bytes);
                samples += 1;
            }
            json!({"before":before,"after":win32::resources(std::process::id()).unwrap(),
                "sampled_max_private_bytes":maximum,"samples":samples,"interval_ms":100,
                "scope":"native test process only; GPU worker RAM and VRAM are not included"})
        });
        Self { stop, handle }
    }
    fn finish(self) -> Value {
        self.stop.store(true, Ordering::Release);
        self.handle.join().unwrap()
    }
}

fn save(path: &Path, report: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
}
fn path_env(key: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(key).unwrap_or_else(|| panic!("{key} required")));
    assert!(path.is_absolute());
    path
}
fn pcm_hash(samples: &[i16]) -> String {
    let mut digest = Sha256::new();
    for sample in samples {
        digest.update(sample.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}
fn batch(pipeline: &Pipeline, settings: &Settings, samples: &[i16]) -> Transcript {
    let cancellation = OperationCancellation::default();
    let permit = pipeline.scheduler().acquire(true, &cancellation).unwrap();
    pipeline
        .stt()
        .transcribe_cancellable_with_options(
            samples,
            &settings.language,
            permit.cancellation.clone(),
            settings.decoding_options(),
        )
        .unwrap()
}
fn activate(
    pipeline: &Pipeline,
    settings: &Settings,
    vad: &Path,
    workers: &WorkerPaths,
    calls: Arc<Mutex<Vec<Value>>>,
    started: Instant,
) -> u64 {
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    assert!(pipeline.set_session_settings_for(operation, settings.clone()));
    let engine = pipeline.stt().clone();
    let settings = settings.clone();
    let workers = workers.clone();
    pipeline.start_capture_segments_for(operation, vad.to_owned(), START_AFTER_SAMPLES,
        move |samples, start, context, cancelled| {
            let begin = started.elapsed().as_secs_f64();
            engine.ensure_model_loaded(&settings.selected_stt_model().unwrap(), settings.acceleration, &workers, cancelled)?;
            let context = context.filter(|_| engine.capabilities().is_some_and(|c| c.initial_prompt));
            let result = engine.transcribe_window_with_options(samples, &settings.language, context, operation,
                cancelled, start, settings.decoding_options());
            calls.lock().push(json!({"start":start,"end":start+samples.len() as u64,
                "context_bytes":context.map_or(0,str::len),"started_seconds":begin,
                "finished_seconds":started.elapsed().as_secs_f64(),"cancelled":cancelled.is_cancelled()}));
            result
        }).unwrap();
    operation
}

fn cancellation(
    pipeline: &Pipeline,
    writer: &Arc<Mutex<Option<RecordingWriter>>>,
    settings: &Settings,
    samples: &[i16],
    vad: &Path,
) -> Value {
    let operation = pipeline
        .start_recording_from(None, OperationSource::Ui)
        .unwrap();
    pipeline.set_session_settings_for(operation, settings.clone());
    let engine = pipeline.stt().clone();
    let settings = settings.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let active = Arc::new(AtomicUsize::new(0));
    let running = active.clone();
    pipeline
        .start_capture_segments_for(
            operation,
            vad.to_owned(),
            START_AFTER_SAMPLES,
            move |samples, start, context, cancelled| {
                running.fetch_add(1, Ordering::AcqRel);
                let _ = entered_tx.send(());
                let result = engine.transcribe_window_with_options(
                    samples,
                    &settings.language,
                    context,
                    operation,
                    cancelled,
                    start,
                    settings.decoding_options(),
                );
                running.fetch_sub(1, Ordering::AcqRel);
                result
            },
        )
        .unwrap();
    assert!(!writer
        .lock()
        .as_ref()
        .unwrap()
        .lock()
        .append(samples, 300 * 16000));
    entered_rx.recv_timeout(Duration::from_secs(15)).unwrap();
    thread::sleep(Duration::from_millis(20));
    let was_active = active.load(Ordering::Acquire) > 0;
    pipeline.stop_recording_for(operation).unwrap();
    let task = pipeline.take_capture_segments_for(operation).unwrap();
    let started = Instant::now();
    pipeline.cancel_for(operation).unwrap();
    assert!(matches!(task.finish(), Err(AppError::Cancelled(_))));
    let joined_seconds = started.elapsed().as_secs_f64();
    assert_eq!(active.load(Ordering::Acquire), 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while crate::application::updates::activity::is_busy() {
        assert!(
            Instant::now() < deadline,
            "cancel cleanup still owns activity"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!pipeline.scheduler().is_busy());
    let before = win32::cpu_seconds(std::process::id()).unwrap();
    thread::sleep(Duration::from_millis(100));
    json!({"recognizer_active_at_cancel":was_active,"join_seconds":joined_seconds,
        "cleanup_seconds":started.elapsed().as_secs_f64(),"active_inferences_after_join":0,
        "scheduler_busy_after_join":false,"activity_busy_after_cleanup":false,
        "idle_100ms_cpu_seconds":win32::cpu_seconds(std::process::id()).unwrap()-before})
}

#[test]
#[ignore = "explicit bounded FONO-124 long public-WAV replay; pinned Tiny and optional Vulkan worker"]
fn long_dictation_public_replay() {
    win32::protect_descendants().unwrap();
    win32::watch_runner(
        std::env::var("FONO_LONG_RUNNER_PID")
            .unwrap()
            .parse()
            .unwrap(),
        240,
    )
    .unwrap();
    let corpus_path = path_env("FONO_LONG_CORPUS");
    let output = path_env("FONO_LONG_OUTPUT");
    let model = SttModelSelection::from_whisper_path(
        path_env("FONO_LONG_MODEL").to_string_lossy().into_owned(),
    );
    let acceleration: AccelerationMode = serde_json::from_str(&format!(
        "\"{}\"",
        std::env::var("FONO_LONG_BACKEND").unwrap()
    ))
    .unwrap();
    assert!(matches!(
        acceleration,
        AccelerationMode::Cpu | AccelerationMode::Vulkan
    ));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vad = root.join("resources/vad/silero_vad.onnx");
    let workers = WorkerPaths::from_resource_dir(root).with_speech_vad(vad.clone());
    let recorder = Replay::default();
    let writer = recorder.0.clone();
    let pipeline = Pipeline::new_with_test_audio_owner(Box::new(recorder));
    let corpus: Value = serde_json::from_slice(&std::fs::read(&corpus_path).unwrap()).unwrap();
    let mut settings = Settings {
        stt_model: Some(model.clone()),
        acceleration,
        whisper_threads: Some(8),
        pretranscription_enabled: true,
        ..Settings::default()
    };
    pipeline
        .stt()
        .ensure_model_loaded(
            &model,
            acceleration,
            &workers,
            &OperationCancellation::default(),
        )
        .unwrap();
    let device = pipeline.stt().device();
    assert_eq!(
        device,
        if acceleration == AccelerationMode::Cpu {
            "CPU"
        } else {
            "Vulkan"
        }
    );
    let mut report = json!({"schema":1,"device":device,"model":model.id,"profile":"debug",
        "policy":{"threads":8,"start_after_samples":START_AFTER_SAMPLES,"feed_samples":1600,
            "feed_interval_ms":100,"stop_ready_scope":"native recognizer text; excludes LLM, UI and insertion"},
        "entries":[],"completed":false});
    save(&output, &report);
    let mut first = None;
    for entry in corpus["entries"].as_array().unwrap() {
        let wav = corpus_path
            .parent()
            .unwrap()
            .join(entry["path"].as_str().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&wav).unwrap())),
            entry["sha256"]
        );
        let audio =
            crate::application::audio_ingest::decode_file(&wav, Default::default()).unwrap();
        let samples = audio.pcm_samples;
        assert_eq!(samples.len(), 60 * 16000);
        assert_eq!(
            (audio.source_sample_rate, audio.source_channels),
            (16000, 1)
        );
        settings.language = entry["language"].as_str().unwrap().into();
        if first.is_none() {
            batch(&pipeline, &settings, &samples[..5 * 16000]); // warm inference, never scored
            first = Some((settings.clone(), samples.clone()));
        }
        eprintln!("long replay {device}: {}", entry["id"]);
        let mut gate = fono_audio::StreamingSpeechVad::new(&vad).unwrap();
        let memory = Memory::start();
        let started = Instant::now();
        assert!(
            gate.analyze_recording(&samples, || false)
                .unwrap()
                .has_speech
        );
        let baseline = batch(&pipeline, &settings, &samples);
        let baseline_seconds = started.elapsed().as_secs_f64();
        let baseline_memory = memory.finish();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let memory = Memory::start();
        let started = Instant::now();
        let operation = activate(&pipeline, &settings, &vad, &workers, calls.clone(), started);
        let (service_tx, service_rx) = mpsc::channel();
        let scheduler = pipeline.scheduler();
        let service = thread::spawn(move || {
            let queued = Instant::now();
            let _permit = scheduler
                .acquire(false, &OperationCancellation::default())
                .unwrap();
            service_tx.send(queued.elapsed().as_secs_f64()).unwrap();
        });
        for (index, chunk) in samples.chunks(1600).enumerate() {
            assert!(!writer
                .lock()
                .as_ref()
                .unwrap()
                .lock()
                .append(chunk, 300 * 16000));
            let deadline = started + Duration::from_millis((index as u64 + 1) * 100);
            if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(delay);
            }
        }
        let fed_seconds = started.elapsed().as_secs_f64();
        let at_stop = pipeline.completed_capture_segments_for(operation).unwrap();
        let stop = Instant::now();
        let captured = pipeline.stop_recording_for(operation).unwrap().unwrap();
        assert_eq!(captured, samples);
        // Ordinary Stop admits the complete recording before recognition.
        // Keep this cost in both variants, including the pretranscribed one.
        assert!(
            gate.analyze_recording(&captured, || false)
                .unwrap()
                .has_speech
        );
        let prefix = pipeline
            .take_capture_segments_for(operation)
            .unwrap()
            .finish()
            .unwrap();
        let join_seconds = stop.elapsed().as_secs_f64();
        let processed_until = prefix.processed_until;
        let prefix_segments = prefix
            .results
            .iter()
            .map(|r| {
                json!({"start":r.range.start,"end":r.range.end,
            "speech":r.transcript.is_some()})
            })
            .collect::<Vec<_>>();
        let cancellation = pipeline.cancellation(operation).unwrap();
        let result = Runtime {
            stt: pipeline.stt(),
            scheduler: &pipeline.scheduler(),
        }
        .finish(
            operation,
            &settings,
            &samples,
            prefix,
            &cancellation,
            || Ok((vad.clone(), workers.clone())),
        );
        let (transcript, fallback) = match result {
            Ok(transcript) => (transcript, false),
            Err(_) => (batch(&pipeline, &settings, &samples), true),
        };
        let stop_ready_seconds = stop.elapsed().as_secs_f64();
        // Audit ranges after timing; Runtime::finish already plans the tail.
        let tail = tail_ranges(&samples, processed_until, &vad, &cancellation).unwrap();
        assert!(
            service_rx.try_recv().is_err(),
            "service overtook the interactive reservation"
        );
        pipeline
            .finish_operation(operation, TerminalReason::Completed)
            .unwrap();
        let service_wait_seconds = service_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        service.join().unwrap();
        let capture_memory = memory.finish();
        report["entries"].as_array_mut().unwrap().push(json!({"id":entry["id"],"language":entry["language"],
            "pcm_sha256":pcm_hash(&samples),"samples":samples.len(),"operation_id":operation,
            "baseline":{"text":baseline.text,"device":baseline.device,"stop_ready_seconds":baseline_seconds,"memory":baseline_memory},
            "capture":{"text":transcript.text,"device":transcript.device,"stop_ready_seconds":stop_ready_seconds,
                "feed_seconds":fed_seconds,"join_seconds":join_seconds,"completed_at_stop":at_stop,
                "processed_until":processed_until,"remaining_seconds":(samples.len()-processed_until) as f64/16000.0,
                "prefix":prefix_segments,"tail":tail.iter().map(|r|json!({"start":r.start,"end":r.end,"forced":r.forced})).collect::<Vec<_>>(),
                "calls":*calls.lock(),"full_batch_fallback":fallback,"memory":capture_memory},
            "service_scheduler":{"entered_before_interactive_finish":false,"wait_seconds":service_wait_seconds}}));
        save(&output, &report);
    }
    let (settings, samples) = first.unwrap();
    report["cancel"] = cancellation(&pipeline, &writer, &settings, &samples, &vad);
    report["completed"] = json!(true);
    save(&output, &report);
}
