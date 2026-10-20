//! CPU-only NeMo RNNT/TDT using the same shared sherpa/ORT as speech VAD.
use super::{
    offline_models::{self, OfflineModelSpec},
    recognizer::{
        self, CancellationSupport, RecognitionOptions, Recognizer, RecognizerCapabilities,
        RecognizerLoad, WindowOptions,
    },
};
use crate::{
    error::{AppError, AppResult},
    types::{ModelId, Transcript},
};
use fono_core::OperationCancellation;
use fono_stt_protocol::{BackendKind, TimedPiece, TimedSegment, WindowTranscript};
use parking_lot::Mutex;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};

const MAXIMUM: usize = fono_audio::speech_segments::DEFAULT_MAX_SEGMENT_SAMPLES;

pub(super) struct SherpaRecognizer {
    engine: Mutex<OfflineRecognizer>,
    // Drop the native context before releasing the verified file handles.
    _files: Vec<File>,
    spec: OfflineModelSpec,
    vad_model: PathBuf,
}

impl SherpaRecognizer {
    pub(super) fn is_idle(&self) -> bool {
        self.engine.try_lock().is_some()
    }

    fn decode(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        if samples.len() > MAXIMUM {
            return Err(AppError::Stt("offline segment exceeds 25 seconds".into()));
        }
        if options.recognition.language != "auto"
            && !self
                .spec
                .languages
                .iter()
                .any(|language| language == options.recognition.language)
        {
            return Err(AppError::Config(
                "Language is not supported by the selected offline model".into(),
            ));
        }
        let mut output = WindowTranscript {
            text: String::new(),
            segments: Vec::new(),
            words: Vec::new(),
            detected_language: None,
            transcribe_secs: 0.0,
            audio_secs: samples.len() as f32 / 16_000.0,
            backend: BackendKind::Cpu,
        };
        recognizer::check_cancellation(cancellation)?;
        if samples.is_empty() {
            return Ok(output);
        }
        let engine = self.engine.lock();
        recognizer::check_cancellation(cancellation)?;
        let started = Instant::now();
        let pcm: Vec<f32> = samples
            .iter()
            .map(|sample| *sample as f32 / 32768.0)
            .collect();
        let stream = engine.create_stream();
        stream.accept_waveform(16_000, &pcm);
        engine.decode(&stream);
        recognizer::check_cancellation(cancellation)?;
        let result = stream
            .get_result()
            .ok_or_else(|| AppError::Stt("Offline recognizer returned no result".into()))?;
        output.transcribe_secs = started.elapsed().as_secs_f32();
        output.text = result.text.trim().to_owned();
        output.words = timed_words(
            &result.tokens,
            result.timestamps.as_deref(),
            result.durations.as_deref(),
            options.audio_start_sample,
            samples.len(),
        )?;
        if !output.text.is_empty() {
            output.segments.push(TimedSegment {
                text: output.text.clone(),
                start_sample: options.audio_start_sample,
                end_sample: options
                    .audio_start_sample
                    .saturating_add(samples.len() as u64),
            });
        }
        Ok(output)
    }
}

impl Recognizer for SherpaRecognizer {
    fn load(request: &RecognizerLoad<'_>) -> AppResult<Self> {
        let spec = offline_models::specification(&request.model.id)?;
        if sherpa_onnx::version().trim_start_matches('v') != "1.13.4" {
            return Err(AppError::Config(
                "Offline STT requires the pinned shared sherpa-onnx 1.13.4 runtime".into(),
            ));
        }
        let directory = Path::new(&request.model.local_path);
        let files = offline_models::verify_directory(directory, &spec, request.cancellation)?;
        validate_tokens(&directory.join(&spec.tokens))?;
        let vad_model = request
            .speech_vad
            .ok_or_else(|| AppError::Config("Shared VAD model is required for offline STT".into()))?
            .to_owned();
        if !vad_model.is_file() {
            return Err(AppError::ModelFileMissing);
        }
        let path = |filename: &str| Some(directory.join(filename).to_string_lossy().into_owned());
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.sample_rate = 16_000;
        config.feat_config.feature_dim = spec.feature_dim;
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: path(&spec.encoder),
            decoder: path(&spec.decoder),
            joiner: path(&spec.joiner),
        };
        config.model_config.tokens = path(&spec.tokens);
        config.model_config.provider = Some("cpu".into());
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.num_threads = 2;
        config.decoding_method = Some("greedy_search".into());
        recognizer::check_cancellation(request.cancellation)?;
        let engine = OfflineRecognizer::create(&config).ok_or_else(|| {
            AppError::Stt("Could not initialize the verified offline model".into())
        })?;
        Ok(Self {
            engine: Mutex::new(engine),
            _files: files,
            spec,
            vad_model,
        })
    }

    fn model(&self) -> &ModelId {
        &self.spec.id
    }
    fn device(&self) -> &'static str {
        "CPU"
    }
    fn capabilities(&self) -> RecognizerCapabilities {
        RecognizerCapabilities {
            max_segment_samples: MAXIMUM,
            window_transcription: true,
            initial_prompt: false,
            hotwords: false,
            language_detection: false,
            automatic_language: true,
            token_timestamps: true,
            decoding_options: false,
            cancellation: CancellationSupport::BetweenSegments,
        }
    }
    fn transcribe(
        &self,
        samples: &[i16],
        options: RecognitionOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<Transcript> {
        let segments = fono_audio::speech_segments::segment_recording(
            samples,
            &self.vad_model,
            MAXIMUM,
            || cancellation.is_cancelled(),
        )
        .map_err(|error| AppError::Stt(error.to_string()))?
        .ok_or_else(|| AppError::Cancelled("Offline segmentation cancelled".into()))?;
        let result = crate::application::segmented_transcription::transcribe_segments(
            samples,
            &segments,
            cancellation,
            |segment, start| {
                (
                    self.decode(
                        segment,
                        WindowOptions {
                            recognition: options,
                            context: None,
                            operation_id: 0,
                            audio_start_sample: start,
                        },
                        cancellation,
                    ),
                    false,
                )
            },
        )?;
        Ok(super::window::to_transcript(result))
    }
    fn transcribe_window(
        &self,
        samples: &[i16],
        options: WindowOptions<'_>,
        cancellation: &OperationCancellation,
    ) -> AppResult<WindowTranscript> {
        self.decode(samples, options, cancellation)
    }
}

fn validate_tokens(path: &Path) -> AppResult<()> {
    let tokens = std::fs::read_to_string(path)?;
    let mut count = 0;
    for (index, line) in tokens.lines().enumerate() {
        let (_, id) = line
            .rsplit_once(' ')
            .ok_or_else(|| AppError::Config("Invalid offline tokenizer".into()))?;
        if id.parse::<usize>().ok() != Some(index) {
            return Err(AppError::Config(
                "Offline tokenizer IDs must be contiguous".into(),
            ));
        }
        count += 1;
    }
    if count < 2
        || !tokens
            .lines()
            .last()
            .is_some_and(|line| line.starts_with("<blk> "))
    {
        return Err(AppError::Config(
            "Offline tokenizer blank ID is missing".into(),
        ));
    }
    Ok(())
}

fn timed_words(
    tokens: &[String],
    timestamps: Option<&[f32]>,
    durations: Option<&[f32]>,
    offset: u64,
    length: usize,
) -> AppResult<Vec<TimedSegment>> {
    let Some(times) = timestamps.filter(|times| times.len() == tokens.len()) else {
        return Ok(Vec::new());
    };
    let mut pieces = Vec::with_capacity(tokens.len());
    let mut previous = 0;
    for (index, token) in tokens.iter().enumerate() {
        let time = times[index];
        if !time.is_finite() || time < 0.0 {
            return Err(AppError::Stt(
                "Offline tokenizer returned invalid timestamps".into(),
            ));
        }
        let start = ((time * 16_000.0) as usize).min(length);
        if start < previous {
            return Err(AppError::Stt("Offline timestamps are out of order".into()));
        }
        let end = durations
            .and_then(|durations| durations.get(index))
            .filter(|duration| duration.is_finite() && **duration >= 0.0)
            .map(|duration| ((time + duration) * 16_000.0) as usize)
            .or_else(|| {
                times
                    .get(index + 1)
                    .filter(|time| time.is_finite() && **time >= 0.0)
                    .map(|time| (*time * 16_000.0) as usize)
            })
            .unwrap_or(length)
            .clamp(start, length);
        pieces.push(TimedPiece {
            bytes: token.replace('▁', " ").into_bytes(),
            start_sample: offset.saturating_add(start as u64),
            end_sample: offset.saturating_add(end as u64),
        });
        previous = start;
    }
    fono_stt_protocol::words_from_pieces(&pieces)
        .map_err(|_| AppError::Stt("Invalid offline token text".into()))
}

#[cfg(test)]
#[path = "offline_tests.rs"]
mod tests;
