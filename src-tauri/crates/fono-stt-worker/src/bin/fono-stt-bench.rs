//! Offline runner: no microphone, credentials, app settings, or transcript storage.
#[path = "../framing.rs"]
mod framing;
#[path = "../benchmark/metrics.rs"]
mod metrics;
#[path = "../benchmark/transport.rs"]
mod transport;
#[path = "../benchmark/wav.rs"]
mod wav;
// Keep replay decisions identical to native live dictation.
mod stt {
    pub use fono_stt_protocol::{TimedSegment, WindowTranscript};
}
#[allow(dead_code)]
#[path = "../../../../src/application/live_agreement.rs"]
mod live_agreement;
#[allow(dead_code)]
#[path = "../../../../src/application/live_windows.rs"]
mod live_windows;

use fono_stt_protocol::{inference::InferenceState, BackendKind, WindowTranscript};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use whisper_rs::{WhisperContext, WhisperContextParameters};

enum Engine {
    Embedded(InferenceState, BackendKind),
    Worker(transport::Worker),
}
impl Engine {
    fn batch(&mut self, samples: &[i16], language: &str) -> Result<WindowTranscript, String> {
        match self {
            Self::Embedded(engine, backend) => {
                engine.transcribe(samples, language, None, 0, *backend, false, &|| false)
            }
            Self::Worker(worker) => worker.batch(samples, language),
        }
    }
    fn window(
        &mut self,
        samples: &[i16],
        language: &str,
        start: u64,
        context: Option<&str>,
    ) -> Result<WindowTranscript, String> {
        match self {
            Self::Embedded(engine, backend) => {
                engine.transcribe(samples, language, context, start, *backend, true, &|| false)
            }
            Self::Worker(worker) => worker.window(samples, language, start, context),
        }
    }
}

fn argument(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|value| value == name)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().collect();
    let model = argument(&arguments, "--model").ok_or("usage: fono-stt-bench --model path --wav PCM16mono16k.wav [--language ru] [--reference text.txt] [--worker worker.exe] [--cold-replay]")?;
    let cold_replay = arguments.iter().any(|argument| argument == "--cold-replay");
    let wav_path = argument(&arguments, "--wav").ok_or("--wav is required")?;
    let language = argument(&arguments, "--language").unwrap_or("auto".into());
    let reference = argument(&arguments, "--reference")
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    let samples = wav::load(Path::new(&wav_path))?;
    let audio_secs = samples.len() as f64 / 16000.0;
    let load_started = Instant::now();
    let mut engine = if let Some(worker) = argument(&arguments, "--worker") {
        Engine::Worker(transport::Worker::load(&worker, &model)?)
    } else {
        let backend = if cfg!(feature = "cuda") {
            BackendKind::Cuda
        } else if cfg!(feature = "vulkan") {
            BackendKind::Vulkan
        } else {
            BackendKind::Cpu
        };
        let mut params = WhisperContextParameters::default();
        params.use_gpu(backend != BackendKind::Cpu);
        let context =
            WhisperContext::new_with_params(&model, params).map_err(|error| error.to_string())?;
        Engine::Embedded(InferenceState::new(Arc::new(context))?, backend)
    };
    let load_secs = load_started.elapsed().as_secs_f64();
    let probe = &samples[..samples.len().min(20 * 16000)];
    let probes = if cold_replay {
        None
    } else {
        Some((
            engine.window(probe, &language, 0, None)?,
            engine.window(probe, &language, 0, None)?,
        ))
    };
    let mut batch_text = String::new();
    let batch_started = Instant::now();
    if !cold_replay {
        for chunk in samples.chunks(300 * 16000) {
            let result = engine.batch(chunk, &language)?;
            if !batch_text.is_empty() {
                batch_text.push(' ');
            }
            batch_text.push_str(&result.text);
        }
    }
    let batch_wall_secs = batch_started.elapsed().as_secs_f64();
    let mut agreement = live_agreement::LiveAgreement::default();
    let mut end = samples.len().min(16000);
    let mut start = 0_usize;
    let mut virtual_secs = 0.0_f64;
    let mut ttfp = None;
    let mut max_lag = 0.0_f64;
    let mut inference_secs = 0.0_f64;
    let mut processing_secs = 0.0_f64;
    let mut windows = 0;
    let mut first_inference_secs = 0.0;
    loop {
        if end.saturating_sub(start) > 30 * 16000 {
            return Err("replay backlog exceeds 30 seconds; accuracy cannot be claimed".into());
        }
        let window_end = end.min(start + 20 * 16000);
        let bounded = window_end - start == 20 * 16000;
        let final_window = window_end == samples.len();
        let window_started = Instant::now();
        let mut hypothesis =
            engine.window(&samples[start..window_end], &language, start as u64, None)?;
        let wall_secs = window_started.elapsed().as_secs_f64();
        if windows == 0 {
            first_inference_secs = hypothesis.transcribe_secs;
        }
        virtual_secs = virtual_secs.max(end as f64 / 16000.0) + wall_secs;
        processing_secs += wall_secs;
        inference_secs += hypothesis.transcribe_secs as f64;
        windows += 1;
        if bounded && !final_window {
            hypothesis
                .words
                .retain(|word| word.end_sample <= (window_end as u64).saturating_sub(8000));
        }
        let appended = agreement.accept(&hypothesis, window_end as u64, final_window || bounded);
        if arguments.iter().any(|argument| argument == "--trace") {
            eprintln!(
                "{}",
                serde_json::json!({"start_sample":start,"end_sample":window_end,"hypothesis":hypothesis,"appended":appended,"committed":agreement.committed})
            );
        }
        if !appended.is_empty() && ttfp.is_none() {
            ttfp = Some(virtual_secs);
        }
        max_lag = max_lag.max(virtual_secs - agreement.committed_end as f64 / 16000.0);
        if final_window {
            break;
        }
        start = live_windows::next_start(
            start as u64,
            agreement.committed_end,
            None,
            window_end as u64,
            bounded,
        )
        .ok_or("model did not confirm words in a bounded replay window")? as usize;
        end = (end + 16000)
            .max((virtual_secs * 16000.0) as usize)
            .min(samples.len());
    }
    let backend = match &engine {
        Engine::Embedded(_, backend) => *backend,
        Engine::Worker(worker) => worker.backend,
    };
    let cold_inference_secs = probes
        .as_ref()
        .map_or(first_inference_secs, |(cold, _)| cold.transcribe_secs);
    let mut report = serde_json::json!({
        "mode": if cold_replay { "cold_replay" } else { "cold_warm_batch_replay" },
        "backend": backend, "audio_secs": audio_secs, "load_secs": load_secs,
        "cold_inference_secs": cold_inference_secs,
        "cold_start_total_secs": load_secs + cold_inference_secs as f64,
        "probe_audio_secs": if cold_replay {audio_secs.min(1.0)} else {probe.len() as f64 / 16000.0},
        "replay": { "windows": windows, "ttfp_secs": ttfp, "ttfp_from_process_start_secs": ttfp.map(|seconds|load_secs+seconds), "ttfp_origin": "start_of_wav", "warmed":!cold_replay, "ttfp_includes_model_load":false, "maximum_commit_lag_secs": max_lag,
            "inference_secs": inference_secs, "processing_secs": processing_secs, "rtf": processing_secs / audio_secs, "inference_rtf": inference_secs / audio_secs, "text_characters": agreement.committed.chars().count(), "virtual_realtime": true, "endpoint_policy":"explicit_final_only", "prompt_characters":0, "speech_gate":false }
    });
    if let Some((_, warm)) = probes {
        report["warm_inference_secs"] = serde_json::json!(warm.transcribe_secs);
        report["batch"] = serde_json::json!({ "wall_secs": batch_wall_secs, "rtf": batch_wall_secs / audio_secs, "maximum_chunk_seconds": 300 });
        report["replay"]["batch_agreement"] = metrics::errors(&batch_text, &agreement.committed);
    }
    if let Some(reference) = reference {
        if !cold_replay {
            report["batch"]["accuracy"] = metrics::errors(&reference, &batch_text);
        }
        report["replay"]["accuracy"] = metrics::errors(&reference, &agreement.committed);
    }
    if arguments
        .iter()
        .any(|argument| argument == "--include-text")
    {
        if !cold_replay {
            report["batch"]["text"] = serde_json::json!(batch_text);
        }
        report["replay"]["text"] = serde_json::json!(agreement.committed);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
