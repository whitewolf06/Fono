use super::phrase_matcher::{normalize, PhraseMatcher};
use crate::{WakeWordBackend, WakeWordConfig, WakeWordError, WakeWordResult};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};

pub(crate) struct Detection {
    pub phrase: String,
    pub json: String,
    pub phrase_end_seconds: f32,
}

pub(crate) struct SherpaDetector {
    // Stream must be destroyed before its owning recognizer.
    stream: OnlineStream,
    engine: OnlineRecognizer,
    matcher: PhraseMatcher,
    phrase: String,
    sample_rate: u32,
}
impl SherpaDetector {
    pub fn new(config: &WakeWordConfig) -> WakeWordResult<Self> {
        crate::validate_config(config)?;
        let spec = crate::model_spec(config.backend)
            .ok_or(WakeWordError::BackendNotCompiled(config.backend))?;
        for file in spec.files {
            let path = config.model_dir.join(file);
            if !path.is_file() {
                return Err(WakeWordError::ModelNotFound(path));
            }
        }
        let path = |filename: &str| {
            Some(
                config
                    .model_dir
                    .join(filename)
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        let mut online = OnlineRecognizerConfig::default();
        online.feat_config.sample_rate = spec.sample_rate as i32;
        online.model_config.tokens = path("tokens.txt");
        online.model_config.num_threads = 2;
        online.model_config.provider = Some("cpu".into());
        online.decoding_method = Some("greedy_search".into());
        online.enable_endpoint = true;
        online.rule1_min_trailing_silence = 2.4;
        online.rule2_min_trailing_silence = 0.7;
        online.rule3_min_utterance_length = 20.0;
        if config.backend == WakeWordBackend::SherpaStreamingRu {
            online.model_config.t_one_ctc.model = path("model.onnx");
        } else {
            online.model_config.transducer.encoder = path(spec.files[0]);
            online.model_config.transducer.decoder = path(spec.files[1]);
            online.model_config.transducer.joiner = path(spec.files[2]);
        }
        let engine = OnlineRecognizer::create(&online).ok_or_else(|| {
            WakeWordError::ModelLoad("failed to initialize local streaming wake recognizer".into())
        })?;
        let stream = engine.create_stream();
        Ok(Self {
            stream,
            engine,
            matcher: PhraseMatcher::new(
                &config.phrase,
                config.phrase_stability_ms,
                config.phrase_confirmations,
                config.sample_rate,
            ),
            phrase: config.phrase.clone(),
            sample_rate: config.sample_rate,
        })
    }
    pub fn accept(&mut self, samples: &[f32], clock: u64) -> Option<Detection> {
        // Sherpa statefully resamples 16 kHz hub input to T-one's 8 kHz.
        self.stream
            .accept_waveform(self.sample_rate as i32, samples);
        self.decode(clock)
    }
    pub fn finish(&mut self, clock: u64) -> Option<Detection> {
        self.stream.input_finished();
        self.decode(clock)
    }
    fn decode(&mut self, clock: u64) -> Option<Detection> {
        let mut decoded = false;
        while self.engine.is_ready(&self.stream) {
            self.engine.decode(&self.stream);
            decoded = true;
        }
        if !decoded {
            return None;
        }
        let result = self.engine.get_result(&self.stream)?;
        let matched_end = self.matcher.observe(&result.text, clock)?;
        let mut joined = String::new();
        let mut end_seconds = 0.0;
        for (index, token) in result.tokens.iter().enumerate() {
            joined.push_str(token);
            if normalize(&joined).len() >= matched_end {
                end_seconds = result
                    .timestamps
                    .as_ref()
                    .and_then(|timestamps| timestamps.get(index))
                    .copied()
                    .unwrap_or_default();
                break;
            }
        }
        Some(Detection {
            phrase: self.phrase.clone(),
            json: serde_json::json!({"matched_phrase":self.phrase,"stable":true}).to_string(),
            phrase_end_seconds: result.start_time.unwrap_or_default() + end_seconds,
        })
    }
    pub fn is_endpoint(&self) -> bool {
        self.engine.is_endpoint(&self.stream)
    }
    pub fn reset(&mut self) {
        self.stream = self.engine.create_stream();
        self.matcher.reset();
    }
}
