//! Reproducible streaming WAV replay through the same decoder and matcher.
use crate::backend::{
    decoder_session::{DecoderSession, SessionDetection},
    sherpa_detector::SherpaDetector,
};
use crate::{WakeWordConfig, WakeWordError, WakeWordResult};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WakeReplayReport {
    pub audio_samples: usize,
    pub audio_duration_ms: u64,
    pub processing_ms: u64,
    pub model_load_ms: u64,
    pub detections: u32,
    pub first_detection_ms: Option<u64>,
    pub first_phrase_end_ms: Option<u64>,
    pub max_decode_ms: u64,
}

pub fn replay_samples(
    config: &WakeWordConfig,
    samples: &[i16],
) -> WakeWordResult<WakeReplayReport> {
    let input: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    replay_normalized(config, &input, config.sample_rate)
}
pub fn replay_wav(config: &WakeWordConfig, path: &Path) -> WakeWordResult<WakeReplayReport> {
    let wave = sherpa_onnx::Wave::read(&path.to_string_lossy())
        .ok_or_else(|| WakeWordError::Audio(format!("could not read WAV: {}", path.display())))?;
    replay_normalized(config, wave.samples(), wave.sample_rate().max(1) as u32)
}
fn replay_normalized(
    config: &WakeWordConfig,
    samples: &[f32],
    rate: u32,
) -> WakeWordResult<WakeReplayReport> {
    let mut config = config.clone();
    config.sample_rate = rate;
    let loading = std::time::Instant::now();
    let mut detector = DecoderSession::new(SherpaDetector::new(&config)?, rate);
    let model_load_ms = loading.elapsed().as_millis() as u64;
    let started = std::time::Instant::now();
    let mut report = WakeReplayReport {
        audio_samples: samples.len(),
        audio_duration_ms: samples.len() as u64 * 1000 / rate as u64,
        processing_ms: 0,
        model_load_ms,
        detections: 0,
        first_detection_ms: None,
        first_phrase_end_ms: None,
        max_decode_ms: 0,
    };
    let mut clock = 0u64;
    let mut cooldown_until = 0u64;
    let size = (rate / 50).max(1) as usize;
    let tail = vec![0f32; rate as usize];
    for chunk in samples.chunks(size).chain(tail.chunks(size)) {
        clock += chunk.len() as u64;
        let decode_started = std::time::Instant::now();
        if let Some(detection) = detector.accept(chunk, clock) {
            if clock >= cooldown_until {
                record_detection(&mut report, detection, clock, rate);
                cooldown_until = clock + config.cooldown_ms * rate as u64 / 1000;
            }
        }
        report.max_decode_ms = report
            .max_decode_ms
            .max(decode_started.elapsed().as_millis() as u64);
    }
    if let Some(detection) = detector.finish(clock) {
        if clock >= cooldown_until {
            record_detection(&mut report, detection, clock, rate);
        }
    }
    report.processing_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}
fn record_detection(
    report: &mut WakeReplayReport,
    result: SessionDetection,
    clock: u64,
    rate: u32,
) {
    report.detections += 1;
    if report.first_detection_ms.is_none() {
        report.first_detection_ms = Some(clock * 1000 / rate as u64);
        report.first_phrase_end_ms = Some(result.phrase_end_sample * 1000 / rate as u64);
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CorpusItem {
    pub wav: std::path::PathBuf,
    pub positive: bool,
    pub phrase_end_ms: Option<u64>,
    #[serde(default)]
    pub holdout: bool,
}
#[derive(Debug, serde::Serialize)]
pub struct CorpusReport {
    pub positives: u32,
    pub positives_detected: u32,
    pub holdout_positives: u32,
    pub holdout_detected: u32,
    pub negative_hours: f64,
    pub false_detections: u32,
    pub false_detections_per_hour: Option<f64>,
    pub p95_delay_ms: Option<u64>,
    pub processing_ms: u64,
    pub model_load_ms: u64,
}
pub fn evaluate_corpus(
    config: &WakeWordConfig,
    items: &[CorpusItem],
) -> WakeWordResult<CorpusReport> {
    let mut result = CorpusReport {
        positives: 0,
        positives_detected: 0,
        holdout_positives: 0,
        holdout_detected: 0,
        negative_hours: 0.0,
        false_detections: 0,
        false_detections_per_hour: None,
        p95_delay_ms: None,
        processing_ms: 0,
        model_load_ms: 0,
    };
    let mut delays = Vec::new();
    for item in items {
        let report = replay_wav(config, &item.wav)?;
        result.processing_ms += report.processing_ms;
        result.model_load_ms += report.model_load_ms;
        if item.positive {
            result.positives += 1;
            result.positives_detected += u32::from(report.detections > 0);
            if item.holdout {
                result.holdout_positives += 1;
                result.holdout_detected += u32::from(report.detections > 0);
            }
            if let (Some(detected), Some(end)) = (report.first_detection_ms, item.phrase_end_ms) {
                delays.push(detected.saturating_sub(end));
            }
        } else {
            result.negative_hours += report.audio_duration_ms as f64 / 3_600_000.0;
            result.false_detections += report.detections;
        }
    }
    if result.negative_hours > 0.0 {
        result.false_detections_per_hour =
            Some(result.false_detections as f64 / result.negative_hours);
    }
    if !delays.is_empty() {
        delays.sort_unstable();
        result.p95_delay_ms = delays
            .get((delays.len() * 95).div_ceil(100).saturating_sub(1))
            .copied();
    }
    Ok(result)
}
