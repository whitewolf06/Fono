use super::{
    phrase_matcher::{normalize, PhraseMatcher},
    sherpa_onnx::map_sensitivity,
};
use crate::{WakeWordBackend, WakeWordConfig, WakeWordError, WakeWordResult};
use sherpa_onnx::{
    KeywordSpotter, KeywordSpotterConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineStream,
};

pub(crate) struct Detection {
    pub phrase: String,
    pub json: String,
    pub phrase_end_seconds: f32,
}
enum Engine {
    Kws(KeywordSpotter),
    Asr(OnlineRecognizer),
}
pub(crate) struct SherpaDetector {
    // Stream must be destroyed before its owning engine.
    stream: OnlineStream,
    engine: Engine,
    matcher: PhraseMatcher,
    phrase: String,
    sample_rate: u32,
}
impl SherpaDetector {
    pub fn new(config: &WakeWordConfig) -> WakeWordResult<Self> {
        let engine = if matches!(
            config.backend,
            WakeWordBackend::SherpaStreamingRu | WakeWordBackend::SherpaStreamingEn
        ) {
            let spec = crate::model_spec(config.backend).unwrap();
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
            Engine::Asr(OnlineRecognizer::create(&online).ok_or_else(|| {
                WakeWordError::ModelLoad(
                    "failed to initialize local streaming wake recognizer".into(),
                )
            })?)
        } else {
            let mut kws = KeywordSpotterConfig::default();
            let dir = &config.model_dir;
            let path = |name: &str| Some(dir.join(name).to_string_lossy().into_owned());
            for name in kws_files() {
                let file = dir.join(name);
                if !file.is_file() {
                    return Err(WakeWordError::ModelNotFound(file));
                }
            }
            kws.feat_config.sample_rate = config.sample_rate as i32;
            kws.feat_config.feature_dim = 80;
            kws.model_config.transducer.encoder = path(kws_files()[0]);
            kws.model_config.transducer.decoder = path(kws_files()[1]);
            kws.model_config.transducer.joiner = path(kws_files()[2]);
            kws.model_config.tokens = path("tokens.txt");
            kws.model_config.num_threads = 2;
            kws.model_config.provider = Some("cpu".into());
            kws.keywords_threshold = config.threshold.clamp(0.0, 1.0);
            kws.keywords_score = map_sensitivity(config.sensitivity);
            kws.keywords_buf = Some(crate::phrases::sherpa_phrase_to_tokens(&config.phrase)?);
            Engine::Kws(KeywordSpotter::create(&kws).ok_or_else(|| {
                WakeWordError::ModelLoad("failed to initialize keyword spotter".into())
            })?)
        };
        let stream = match &engine {
            Engine::Kws(e) => e.create_stream(),
            Engine::Asr(e) => e.create_stream(),
        };
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
        match &self.engine {
            Engine::Kws(engine) => {
                while engine.is_ready(&self.stream) {
                    engine.decode(&self.stream);
                    if let Some(result) = engine.get_result(&self.stream) {
                        if !result.keyword.trim().is_empty() {
                            return Some(Detection {
                                phrase: self.phrase.clone(),
                                json: result.json,
                                phrase_end_seconds: result.start_time
                                    + result.timestamps.last().copied().unwrap_or_default(),
                            });
                        }
                    }
                }
                None
            }
            Engine::Asr(engine) => {
                let mut decoded = false;
                while engine.is_ready(&self.stream) {
                    engine.decode(&self.stream);
                    decoded = true;
                }
                if !decoded {
                    return None;
                }
                let result = engine.get_result(&self.stream)?;
                let matched_end = self.matcher.observe(&result.text, clock)?;
                let mut joined = String::new();
                let mut end_seconds = 0.0;
                for (index, token) in result.tokens.iter().enumerate() {
                    joined.push_str(token);
                    if normalize(&joined).len() >= matched_end {
                        end_seconds = result
                            .timestamps
                            .as_ref()
                            .and_then(|t| t.get(index))
                            .copied()
                            .unwrap_or_default();
                        break;
                    }
                }
                Some(Detection {
                    phrase: self.phrase.clone(),
                    json: serde_json::json!({"matched_phrase":self.phrase,"stable":true})
                        .to_string(),
                    phrase_end_seconds: result.start_time.unwrap_or_default() + end_seconds,
                })
            }
        }
    }
    pub fn is_endpoint(&self) -> bool {
        match &self.engine {
            Engine::Asr(e) => e.is_endpoint(&self.stream),
            Engine::Kws(_) => false,
        }
    }
    pub fn reset(&mut self) {
        self.stream = match &self.engine {
            Engine::Kws(e) => e.create_stream(),
            Engine::Asr(e) => e.create_stream(),
        };
        self.matcher.reset();
    }
}
pub(crate) fn kws_files() -> [&'static str; 4] {
    [
        "encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "tokens.txt",
    ]
}
