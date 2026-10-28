//! A capture consumer, never an audio callback: Silero inspects fixed windows
//! and complete PCM ranges are handed off synchronously without an output queue.
use super::{confirmed_pause_end, SpeechSegment, SpeechSegmentPlanner, SAMPLE_RATE};
use crate::speech_vad::WINDOW_SAMPLES as WINDOW;
use crate::{AudioError, AudioResult, StreamingSpeechVad};
use std::{collections::VecDeque, path::Path};

const CAPTURE_PAUSE: usize = SAMPLE_RATE * 600 / 1000;

#[derive(Debug, PartialEq, Eq)]
pub struct ClosedSpeechSegment {
    /// Absolute, consecutive sample offsets within this capture (16 kHz mono).
    pub range: SpeechSegment,
    pub samples: Vec<i16>,
}

pub struct StreamingSpeechSegmenter {
    vad: StreamingSpeechVad,
    planner: SpeechSegmentPlanner,
    pending: VecDeque<i16>,
    received: usize,
}

impl StreamingSpeechSegmenter {
    /// Use one instance per operation on a capture-consumer/blocking thread.
    /// The cap has the same 512-sample..30-second limits as file segmentation.
    pub fn new(model_path: &Path, maximum: usize) -> AudioResult<Self> {
        let planner = SpeechSegmentPlanner::new(maximum)?;
        Ok(Self {
            vad: StreamingSpeechVad::new(model_path)?,
            planner,
            pending: VecDeque::with_capacity(maximum + WINDOW - 1),
            received: 0,
        })
    }

    pub fn received_samples(&self) -> usize {
        self.received
    }

    pub fn buffered_samples(&self) -> usize {
        self.pending.len()
    }

    /// Retained PCM is bounded by `maximum + 511`, including a partial neural
    /// window. Silero's own fixed ring and one emitted segment are separate.
    /// The receiver owns each output and must provide bounded backpressure;
    /// this object does not retain previous segments or spawn background work.
    pub fn accept(
        &mut self,
        samples: &[i16],
        mut emit: impl FnMut(ClosedSpeechSegment),
    ) -> AudioResult<()> {
        self.received
            .checked_add(samples.len())
            .ok_or_else(|| AudioError::Audio("capture sample clock overflow".into()))?;
        let mut rest = samples;
        while !rest.is_empty() {
            let count = (WINDOW - self.received % WINDOW).min(rest.len());
            self.pending.extend(&rest[..count]);
            self.received += count;
            rest = &rest[count..];
            if self.received % WINDOW != 0 {
                continue;
            }
            // Always inspect exactly one neural window, even when a caller
            // supplies a whole recording in one packet. Earlier pauses cannot
            // disappear behind a later phrase or a packet ending in silence.
            let mut window = [0; WINDOW];
            for (target, sample) in window
                .iter_mut()
                .zip(self.pending.range(self.pending.len() - WINDOW..))
            {
                *target = *sample;
            }
            let decision = self.vad.accept(&window);
            let closed_end = confirmed_pause_end(
                decision.current_speech,
                self.vad.closed_speech_end_sample().map(|end| end as usize),
                self.received,
                CAPTURE_PAUSE,
            );
            while let Some(range) = self.planner.next_cut(self.received, closed_end)? {
                self.emit_range(range, &mut emit)?;
            }
        }
        Ok(())
    }

    /// Stop consumes the stream and hands off every remaining sample once,
    /// including a final partial window. No zero padding enters recognition.
    /// Cancellation can simply drop the object; it owns no tasks or threads.
    pub fn finish(mut self, mut emit: impl FnMut(ClosedSpeechSegment)) -> AudioResult<()> {
        // The last incomplete neural window may cross a non-aligned hard cap.
        while let Some(range) = self.planner.next_cut(self.received, None)? {
            self.emit_range(range, &mut emit)?;
        }
        if let Some(range) = self.planner.finish(self.received)? {
            self.emit_range(range, &mut emit)?;
        }
        Ok(())
    }

    fn emit_range(
        &mut self,
        range: SpeechSegment,
        emit: &mut impl FnMut(ClosedSpeechSegment),
    ) -> AudioResult<()> {
        let start = self.received - self.pending.len();
        if range.start != start || range.end <= start || range.end > self.received {
            return Err(AudioError::Audio("invalid streaming segment range".into()));
        }
        let samples = self.pending.drain(..range.end - start).collect();
        emit(ClosedSpeechSegment { range, samples });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
