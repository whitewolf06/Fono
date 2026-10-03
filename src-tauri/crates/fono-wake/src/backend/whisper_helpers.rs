use crate::{WakeWordConfig, WakeWordError, WakeWordResult, WakeWordTestResult};
use std::time::Instant;
use whisper_rs::{SamplingStrategy, WhisperContext, WhisperContextParameters};
pub(super) fn load_model(path: &std::path::Path, use_gpu: bool) -> WakeWordResult<WhisperContext> {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(use_gpu);
    WhisperContext::new_with_params(path, params)
        .map_err(|error| WakeWordError::ModelLoad(format!("WhisperContext: {error}")))
}

pub(super) fn gpu_backend_compiled() -> bool {
    cfg!(any(feature = "cuda", feature = "vulkan"))
}

pub fn test_with_samples(
    config: &WakeWordConfig,
    samples: &[i16],
) -> WakeWordResult<WakeWordTestResult> {
    let started = Instant::now();
    let context = load_model(&config.model_dir, config.use_gpu)?;
    let language = if config.phrase.is_ascii() { "en" } else { "ru" };
    let transcript = transcribe(&context, samples, language)?;
    let normalized = normalize_phrase(&transcript);
    let phrase = normalize_phrase(&config.phrase);
    Ok(WakeWordTestResult {
        detected: phrase_matches(&normalized, &phrase),
        keyword: transcript.clone(),
        json: format!(
            "{{\"transcript\":{},\"normalized\":{}}}",
            serde_json::to_string(&transcript).unwrap_or_else(|_| "\"\"".into()),
            serde_json::to_string(&normalized).unwrap_or_else(|_| "\"\"".into())
        ),
        samples: samples.len(),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

pub(super) fn transcribe(
    ctx: &WhisperContext,
    samples: &[i16],
    language: &str,
) -> WakeWordResult<String> {
    let pcm: Vec<f32> = samples
        .iter()
        .map(|&sample| sample as f32 / i16::MAX as f32)
        .collect();
    let mut params = whisper_rs::FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_n_threads(4);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_no_timestamps(true);

    let mut state = ctx
        .create_state()
        .map_err(|error| WakeWordError::Backend(format!("create_state: {error}")))?;
    state
        .full(params, &pcm)
        .map_err(|error| WakeWordError::Backend(format!("full: {error}")))?;

    let mut text = String::new();
    for index in 0..state.full_n_segments() {
        if let Some(segment) = state.get_segment(index) {
            if let Ok(value) = segment.to_str_lossy() {
                let value = value.trim();
                if !value.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(value);
                }
            }
        }
    }
    Ok(text)
}

pub(super) fn has_speech(samples: &[i16], threshold: f32) -> bool {
    let threshold = threshold.max(0.001);
    samples.chunks(1_600).any(|window| {
        let sum: i64 = window
            .iter()
            .map(|&sample| (sample as i64) * (sample as i64))
            .sum();
        let rms = (sum as f32 / window.len().max(1) as f32).sqrt() / i16::MAX as f32;
        rms >= threshold
    })
}

pub(super) fn normalize_phrase(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for character in value.to_lowercase().chars() {
        if character.is_alphanumeric() {
            normalized.push(character);
        } else if character.is_whitespace() && !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    normalized.trim().to_string()
}

pub(super) fn phrase_matches(transcript: &str, phrase: &str) -> bool {
    if transcript.is_empty() || phrase.is_empty() {
        return false;
    }
    if phrase == "hey fono" {
        return has_adjacent_variant(
            transcript,
            &["hey", "she", "hi", "хей"],
            &["fono", "phono", "phone", "фоно", "фона"],
        );
    }
    if phrase == "okay fun" {
        return has_adjacent_variant(
            transcript,
            &["okay", "ok", "okey", "окей"],
            &["fun", "fan", "фан", "фэн"],
        );
    }
    let expected: Vec<&str> = phrase.split_whitespace().collect();
    let actual: Vec<&str> = transcript.split_whitespace().collect();
    actual
        .windows(expected.len())
        .any(|window| window == expected)
}

fn has_adjacent_variant(transcript: &str, prefixes: &[&str], names: &[&str]) -> bool {
    transcript
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| prefixes.contains(&pair[0]) && names.contains(&pair[1]))
}

#[cfg(test)]
mod tests {
    use super::{normalize_phrase, phrase_matches};

    #[test]
    fn default_phrase_accepts_observed_pronunciation() {
        assert!(phrase_matches("she phono", "hey fono"));
        assert!(phrase_matches("hey fono", "hey fono"));
        assert!(!phrase_matches("phone", "hey fono"));
        assert!(!phrase_matches("hey please open fono", "hey fono"));
        assert!(!phrase_matches("fono hey", "hey fono"));
    }

    #[test]
    fn okay_fun_accepts_common_variants() {
        assert!(phrase_matches("okay fun", "okay fun"));
        assert!(phrase_matches("ok fan", "okay fun"));
        assert!(!phrase_matches("have fun", "okay fun"));
        assert!(!phrase_matches("okay now fun", "okay fun"));
    }

    #[test]
    fn normalization_removes_punctuation() {
        assert_eq!(normalize_phrase(" She, PHONO! "), "she phono");
    }

    #[test]
    fn custom_phrase_requires_adjacent_words() {
        assert!(phrase_matches("открой фоно", "открой фоно"));
        assert!(!phrase_matches("открой пожалуйста фоно", "открой фоно"));
        assert!(!phrase_matches("фоно открой", "открой фоно"));
    }
}
