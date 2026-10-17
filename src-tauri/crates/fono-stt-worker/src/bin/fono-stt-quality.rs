//! Explicit public-corpus CPU quality runner; never opens the microphone or app data.
#[path = "../benchmark/metrics.rs"]
mod metrics;
#[path = "../benchmark/wav.rs"]
mod wav;

use fono_stt_protocol::{
    inference::InferenceState, BackendKind, DecodingOptions, DecodingStrategy,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use whisper_rs::{WhisperContext, WhisperContextParameters};

fn decoding_options(arguments: &[String]) -> Result<DecodingOptions, String> {
    let mut options = DecodingOptions::default();
    let mut strategy_set = false;
    let mut threads_set = false;
    let mut arguments = arguments.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--strategy" if !strategy_set => {
                options.strategy = match value.as_str() {
                    "fast" => DecodingStrategy::Fast,
                    "accurate" => DecodingStrategy::Accurate,
                    _ => return Err("strategy must be fast or accurate".into()),
                };
                strategy_set = true;
            }
            "--threads" if !threads_set => {
                options.threads = Some(
                    value
                        .parse()
                        .map_err(|_| "threads must be an integer from 1 to 8")?,
                );
                threads_set = true;
            }
            _ => return Err(format!("unknown or repeated option: {flag}")),
        }
    }
    options.validate()?;
    Ok(options)
}

fn no_speech_result(
    entry: &serde_json::Value,
    language: &str,
    reference: &str,
) -> serde_json::Value {
    serde_json::json!({"id":entry["id"],"language":language,"no_speech":true,
        "text":"","audio_seconds":entry["original_audio_seconds"].as_f64().unwrap_or(0.0),
        "warm_seconds":0.0,"rtf":0.0,"errors":metrics::errors(reference, "")})
}

fn run() -> Result<(), String> {
    if cfg!(any(feature = "cuda", feature = "vulkan")) {
        return Err("quality baseline must be built without GPU features".into());
    }
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() < 3 {
        return Err("usage: fono-stt-quality MODEL.bin explicit-corpus.json [--strategy fast|accurate] [--threads 1..8]".into());
    }
    let decoding = decoding_options(&arguments[3..])?;
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
        if entry["no_speech"].as_bool() == Some(true) {
            // An upstream speech gate rejection is an empty hypothesis. Keep
            // its reference words in WER so false negatives cannot disappear.
            results.push(no_speech_result(entry, language, reference));
            eprintln!("quality: {} (no speech)", entry["id"]);
            continue;
        }
        let samples = wav::load(Path::new(path))?;
        if cold.is_none() {
            let started = Instant::now();
            let transcript = engine.transcribe_with_options(
                &samples,
                language,
                None,
                0,
                BackendKind::Cpu,
                false,
                &[],
                decoding,
            )?;
            cold = Some(
                serde_json::json!({"id":entry["id"], "seconds":started.elapsed().as_secs_f64(),
                "errors":metrics::errors(reference, &transcript.text)}),
            );
        }
        let started = Instant::now();
        let transcript = engine.transcribe_with_options(
            &samples,
            language,
            None,
            0,
            BackendKind::Cpu,
            false,
            &[],
            decoding,
        )?;
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
        "threads":decoding.effective_threads(),"decoding":decoding,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn omitted_quality_options_preserve_the_existing_baseline_policy() {
        assert_eq!(decoding_options(&[]).unwrap(), DecodingOptions::default());
        let options =
            decoding_options(&args(&["--strategy", "accurate", "--threads", "2"])).unwrap();
        assert_eq!(options.strategy, DecodingStrategy::Accurate);
        assert_eq!(options.effective_threads(), 2);
    }

    #[test]
    fn quality_runner_rejects_ambiguous_or_unbounded_measurements() {
        for values in [
            vec!["--strategy"],
            vec!["--strategy", "guess"],
            vec!["--threads", "0"],
            vec!["--threads", "9"],
            vec!["--threads", "2", "--threads", "3"],
            vec!["--strategy", "fast", "--strategy", "accurate"],
            vec!["--unknown", "fast"],
        ] {
            assert!(decoding_options(&args(&values)).is_err());
        }
    }

    #[test]
    fn speech_gate_false_negative_remains_a_measured_empty_hypothesis() {
        let result = no_speech_result(
            &serde_json::json!({"id":"retained","no_speech":true}),
            "en",
            "quiet word",
        );
        assert_eq!(result["id"], "retained");
        assert_eq!(result["text"], "");
        assert_eq!(result["errors"]["reference_words"], 2);
        assert_eq!(result["errors"]["word_errors"], 2);
    }
}
