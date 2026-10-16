//! Explicit public-corpus CPU quality runner; never opens the microphone or app data.
#[path = "../benchmark/metrics.rs"]
mod metrics;
#[path = "../benchmark/wav.rs"]
mod wav;

use fono_stt_protocol::{inference::InferenceState, BackendKind};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use whisper_rs::{WhisperContext, WhisperContextParameters};

fn run() -> Result<(), String> {
    if cfg!(any(feature = "cuda", feature = "vulkan")) {
        return Err("quality baseline must be built without GPU features".into());
    }
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() != 3 {
        return Err("usage: fono-stt-quality MODEL.bin explicit-corpus.json".into());
    }
    let corpus: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&arguments[2]).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let entries = corpus["entries"].as_array().ok_or("entries missing")?;
    if entries.is_empty() {
        return Err("corpus is empty".into());
    }
    let load_started = Instant::now();
    let mut parameters = WhisperContextParameters::default();
    parameters.use_gpu(false);
    let context = WhisperContext::new_with_params(&arguments[1], parameters)
        .map_err(|error| error.to_string())?;
    let mut engine = InferenceState::new(Arc::new(context))?;
    let load_seconds = load_started.elapsed().as_secs_f64();
    let mut results = Vec::new();
    let mut cold = None;
    for entry in entries {
        let path = entry["path"].as_str().ok_or("path missing")?;
        let language = entry["language"].as_str().ok_or("language missing")?;
        let reference = entry["reference"].as_str().ok_or("reference missing")?;
        let samples = wav::load(Path::new(path))?;
        if cold.is_none() {
            let started = Instant::now();
            let transcript =
                engine.transcribe(&samples, language, None, 0, BackendKind::Cpu, false, &[])?;
            cold = Some(
                serde_json::json!({"id":entry["id"], "seconds":started.elapsed().as_secs_f64(),
                "errors":metrics::errors(reference, &transcript.text)}),
            );
        }
        let started = Instant::now();
        let transcript =
            engine.transcribe(&samples, language, None, 0, BackendKind::Cpu, false, &[])?;
        let seconds = started.elapsed().as_secs_f64();
        let audio_seconds = samples.len() as f64 / 16000.0;
        results.push(serde_json::json!({"id":entry["id"],"language":language,
            "text":transcript.text,"audio_seconds":audio_seconds,"warm_seconds":seconds,
            "rtf":seconds/audio_seconds,"errors":metrics::errors(reference,&transcript.text)}));
        eprintln!("quality: {}", entry["id"]);
    }
    println!(
        "{}",
        serde_json::json!({"schema":1,"backend":"cpu","model_load_seconds":load_seconds,
        "threads":std::thread::available_parallelism().map_or(4, |count| count.get()).clamp(1,8),
        "cold":cold,"entries":results})
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("quality benchmark failed: {error}");
        std::process::exit(1);
    }
}
