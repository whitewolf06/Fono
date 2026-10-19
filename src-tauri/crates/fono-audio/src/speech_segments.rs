//! Consecutive, non-overlapping PCM ranges shared by file and future capture
//! segmentation. Neural VAD chooses pauses; a hard cap bounds continuous speech.
use crate::{AudioError, AudioResult};

pub const SAMPLE_RATE: usize = 16_000;
pub const DEFAULT_MAX_SEGMENT_SAMPLES: usize = 25 * SAMPLE_RATE;
const PAUSE_PADDING: usize = SAMPLE_RATE * 175 / 1000;
#[cfg(feature = "speech-vad")]
// Preserve the already measured completed-file policy. Capture waits 600 ms
// using this same pause predicate and planner, without changing file results.
const MIN_CONFIRMED_PAUSE: usize = SAMPLE_RATE * 300 / 1000;

#[cfg(feature = "speech-vad")]
mod streaming;
#[cfg(feature = "speech-vad")]
pub use streaming::{ClosedSpeechSegment, StreamingSpeechSegmenter};

#[cfg(feature = "speech-vad")]
fn confirmed_pause_end(
    speaking: bool,
    closed_end: Option<usize>,
    inspected: usize,
    minimum_pause: usize,
) -> Option<usize> {
    closed_end.filter(|end| !speaking && inspected.saturating_sub(*end) >= minimum_pause)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSegment {
    pub start: usize,
    pub end: usize,
    pub forced: bool,
}

pub struct SpeechSegmentPlanner {
    start: usize,
    inspected: usize,
    maximum: usize,
}

impl SpeechSegmentPlanner {
    pub fn new(maximum: usize) -> AudioResult<Self> {
        if !(512..=30 * SAMPLE_RATE).contains(&maximum) {
            return Err(AudioError::Audio(
                "segment cap must be 512 samples to 30 seconds".into(),
            ));
        }
        Ok(Self {
            start: 0,
            inspected: 0,
            maximum,
        })
    }

    /// Call until None when feeding more than one maximum-sized block.
    /// The closed neural end already implies 350 ms of confirming silence.
    pub fn next_cut(
        &mut self,
        inspected: usize,
        closed_speech_end: Option<usize>,
    ) -> AudioResult<Option<SpeechSegment>> {
        if inspected < self.inspected {
            return Err(AudioError::Audio("out-of-order segmentation input".into()));
        }
        self.inspected = inspected;
        let limit = self.start.saturating_add(self.maximum);
        let pause = closed_speech_end
            .filter(|end| *end > self.start)
            .and_then(|end| end.checked_add(PAUSE_PADDING))
            .filter(|cut| *cut > self.start && *cut <= inspected && *cut <= limit);
        let (end, forced) = match pause {
            Some(end) => (end, false),
            None if limit <= inspected => (limit, true),
            None => return Ok(None),
        };
        let segment = SpeechSegment {
            start: self.start,
            end,
            forced,
        };
        self.start = end;
        Ok(Some(segment))
    }

    pub fn finish(&mut self, length: usize) -> AudioResult<Option<SpeechSegment>> {
        if length < self.inspected || length < self.start || length - self.start > self.maximum {
            return Err(AudioError::Audio(
                "invalid or unbounded final segment".into(),
            ));
        }
        let segment = (length > self.start).then_some(SpeechSegment {
            start: self.start,
            end: length,
            forced: false,
        });
        self.start = length;
        self.inspected = length;
        Ok(segment)
    }
}

#[cfg(feature = "speech-vad")]
pub fn segment_recording(
    samples: &[i16],
    model_path: &std::path::Path,
    maximum: usize,
    cancelled: impl Fn() -> bool,
) -> AudioResult<Option<Vec<SpeechSegment>>> {
    if cancelled() {
        return Ok(None);
    }
    let mut planner = SpeechSegmentPlanner::new(maximum)?;
    if samples.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let mut vad = crate::StreamingSpeechVad::new(model_path)?;
    let mut segments = Vec::new();
    let mut inspected = 0;
    for chunk in samples.chunks(512) {
        if cancelled() {
            return Ok(None);
        }
        let decision = vad.accept(chunk);
        inspected += chunk.len();
        // Silero also closes a segment at its maximum speech duration. Such
        // a forced close is not evidence of silence and must not be a pause.
        let closed_end = confirmed_pause_end(
            decision.current_speech,
            vad.closed_speech_end_sample()
                .map(|end| (end as usize).min(samples.len())),
            inspected,
            MIN_CONFIRMED_PAUSE,
        );
        while let Some(segment) = planner.next_cut(inspected, closed_end)? {
            segments.push(segment);
        }
    }
    if cancelled() {
        return Ok(None);
    }
    if let Some(segment) = planner.finish(samples.len())? {
        segments.push(segment);
    }
    Ok(Some(segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "speech-vad")]
    #[test]
    fn a_forced_neural_close_is_not_misreported_as_a_silent_boundary() {
        let end = 30 * SAMPLE_RATE;
        assert_eq!(
            confirmed_pause_end(true, Some(end), end + SAMPLE_RATE, MIN_CONFIRMED_PAUSE),
            None
        );
        assert_eq!(
            confirmed_pause_end(false, Some(end), end + 512, MIN_CONFIRMED_PAUSE),
            None
        );
        assert_eq!(
            confirmed_pause_end(false, Some(end), end + SAMPLE_RATE / 2, MIN_CONFIRMED_PAUSE),
            Some(end)
        );
    }

    #[test]
    fn continuous_speech_and_partial_tail_have_exactly_one_owner_per_sample() {
        let length = 71 * SAMPLE_RATE + 137;
        let mut planner = SpeechSegmentPlanner::new(DEFAULT_MAX_SEGMENT_SAMPLES).unwrap();
        let mut ranges = Vec::new();
        while let Some(range) = planner.next_cut(length, None).unwrap() {
            ranges.push(range);
        }
        ranges.extend(planner.finish(length).unwrap());
        assert_eq!(ranges.len(), 3);
        let mut next = 0;
        for range in ranges {
            assert_eq!(range.start, next);
            assert!(range.end - range.start <= DEFAULT_MAX_SEGMENT_SAMPLES);
            next = range.end;
        }
        assert_eq!(next, length);
        assert_eq!(planner.finish(length).unwrap(), None);
    }

    #[test]
    fn neural_pause_wins_and_a_stale_closed_segment_never_cuts_twice() {
        let mut planner = SpeechSegmentPlanner::new(DEFAULT_MAX_SEGMENT_SAMPLES).unwrap();
        let cut = planner
            .next_cut(5 * SAMPLE_RATE, Some(4 * SAMPLE_RATE))
            .unwrap()
            .unwrap();
        assert!(!cut.forced);
        assert_eq!(cut.end, 4 * SAMPLE_RATE + PAUSE_PADDING);
        assert_eq!(
            planner
                .next_cut(5 * SAMPLE_RATE, Some(4 * SAMPLE_RATE))
                .unwrap(),
            None
        );
        assert!(planner.next_cut(3 * SAMPLE_RATE, None).is_err());
    }

    #[cfg(feature = "speech-vad")]
    #[test]
    fn long_public_wav_is_partitioned_without_copying_or_trimming_pcm() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let wav = sherpa_onnx::Wave::read(
            &root
                .join("tests/fixtures/stt-quality/ru-01.wav")
                .to_string_lossy(),
        )
        .unwrap();
        let mut samples = Vec::new();
        for _ in 0..4 {
            samples.extend(wav.samples().iter().map(|sample| (sample * 32767.0) as i16));
            samples.extend(std::iter::repeat(0).take(SAMPLE_RATE));
        }
        let model = root.join("resources/vad/silero_vad.onnx");
        let segments = segment_recording(&samples, &model, DEFAULT_MAX_SEGMENT_SAMPLES, || false)
            .unwrap()
            .unwrap();
        assert!(segments.len() >= 3);
        assert!(segments
            .iter()
            .any(|segment| !segment.forced && segment.end < samples.len()));
        let mut next = 0;
        for segment in segments {
            assert_eq!(segment.start, next);
            assert!(
                segment.end > segment.start
                    && segment.end - segment.start <= DEFAULT_MAX_SEGMENT_SAMPLES
            );
            next = segment.end;
        }
        assert_eq!(next, samples.len());
        assert_eq!(
            segment_recording(&samples, &model, DEFAULT_MAX_SEGMENT_SAMPLES, || true).unwrap(),
            None
        );
    }
}
