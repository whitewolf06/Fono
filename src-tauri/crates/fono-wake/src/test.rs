//! Offline test helpers for wake word models.

/// Result of an offline wake-word test on a WAV file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeWordTestResult {
    pub detected: bool,
    pub keyword: String,
    pub json: String,
    pub samples: usize,
    pub duration_ms: u64,
}

#[cfg(feature = "sherpa-wake")]
pub use sherpa_impl::test_with_wav;

#[cfg(feature = "sherpa-wake")]
mod sherpa_impl {
    use std::path::Path;

    use crate::backend::sherpa_onnx::{map_sensitivity, phrase_to_tokens};
    use crate::config::WakeWordConfig;
    use crate::error::{WakeWordError, WakeWordResult};
    use crate::test::WakeWordTestResult;

    /// Run the KWS spotter on a mono WAV file.
    ///
    /// If `use_builtin_keywords` is true, the keyword list supplied for the
    /// model's bundled test WAVs is used instead of the user's phrase. This is
    /// useful to verify that the model and spotter pipeline work independently
    /// of microphone capture.
    pub fn test_with_wav(
        config: &WakeWordConfig,
        wav_path: &Path,
        use_builtin_keywords: bool,
    ) -> WakeWordResult<WakeWordTestResult> {
        let wave = sherpa_onnx::Wave::read(&wav_path.to_string_lossy()).ok_or_else(|| {
            WakeWordError::ModelLoad(format!("failed to read wav: {}", wav_path.display()))
        })?;

        let samples = wave.samples();
        let sample_rate = wave.sample_rate();
        let duration_ms = (samples.len() as f64 / sample_rate.max(1) as f64 * 1000.0) as u64;

        let dir = &config.model_dir;
        let encoder = dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let decoder = dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let joiner = dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let tokens = dir.join("tokens.txt");

        let mut spotter_config = sherpa_onnx::KeywordSpotterConfig::default();
        spotter_config.feat_config.sample_rate = config.sample_rate as i32;
        spotter_config.feat_config.feature_dim = 80;
        spotter_config.model_config.transducer.encoder =
            Some(encoder.to_string_lossy().into_owned());
        spotter_config.model_config.transducer.decoder =
            Some(decoder.to_string_lossy().into_owned());
        spotter_config.model_config.transducer.joiner = Some(joiner.to_string_lossy().into_owned());
        spotter_config.model_config.tokens = Some(tokens.to_string_lossy().into_owned());
        spotter_config.model_config.num_threads = 2;
        spotter_config.model_config.provider = Some("cpu".into());
        spotter_config.keywords_threshold = config.threshold.clamp(0.0, 1.0);
        spotter_config.keywords_score = map_sensitivity(config.sensitivity);
        if use_builtin_keywords {
            // `test_wavs/0.wav` says "LIGHT UP". That phrase lives in the
            // test-specific list, not in the model's root `keywords.txt`.
            let test_keywords_file = dir.join("test_wavs").join("test_keywords.txt");
            let keywords_file = if test_keywords_file.is_file() {
                test_keywords_file
            } else {
                dir.join("keywords.txt")
            };
            spotter_config.keywords_file = Some(keywords_file.to_string_lossy().into_owned());
            spotter_config.keywords_buf = None;
        } else {
            spotter_config.keywords_file = None;
            spotter_config.keywords_buf = Some(phrase_to_tokens(&config.phrase));
        }

        let spotter = sherpa_onnx::KeywordSpotter::create(&spotter_config)
            .ok_or_else(|| WakeWordError::ModelLoad("failed to create keyword spotter".into()))?;

        let stream = spotter.create_stream();
        stream.accept_waveform(sample_rate, samples);

        // Streaming KWS needs a short tail after the last spoken phoneme so
        // the decoder can finalize a keyword near the end of a WAV file.
        let tail_padding = vec![0.0_f32; (sample_rate.max(1) as usize * 2) / 3];
        stream.accept_waveform(sample_rate, &tail_padding);
        stream.input_finished();

        let mut keyword = String::new();
        let mut json = String::new();
        while spotter.is_ready(&stream) {
            spotter.decode(&stream);
            if let Some(result) = spotter.get_result(&stream) {
                let candidate = result.keyword.trim();
                if !candidate.is_empty() {
                    keyword = candidate.to_string();
                    json = result.json.trim().to_string();
                    break;
                }
            }
        }

        Ok(WakeWordTestResult {
            detected: !keyword.is_empty(),
            keyword,
            json,
            samples: samples.len(),
            duration_ms,
        })
    }
}
