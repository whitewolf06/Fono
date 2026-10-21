//! Optional local trainer data, erased together with the analytics consent.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechTimingSummary {
    pub schema_version: u16,
    pub speech_time_ms: u64,
    pub pause_count: u32,
    pub pause_time_ms: u64,
    pub max_pause_ms: u64,
}
impl SpeechTimingSummary {
    pub fn from_samples(value: fono_audio::SpeechTimingSamples) -> Option<Self> {
        let result = Self {
            schema_version: 1,
            speech_time_ms: value.speech / 16,
            pause_count: value.pause_count,
            pause_time_ms: value.pauses / 16,
            max_pause_ms: value.max_pause / 16,
        };
        result.valid().then_some(result)
    }
    pub fn valid(&self) -> bool {
        self.schema_version == 1
            && self.speech_time_ms > 0
            && self
                .speech_time_ms
                .checked_add(self.pause_time_ms)
                .is_some_and(|total| total <= 86_400_000)
            && self.max_pause_ms <= self.pause_time_ms
            && if self.pause_count == 0 {
                self.pause_time_ms == 0 && self.max_pause_ms == 0
            } else {
                self.max_pause_ms > 0 && self.pause_time_ms >= u64::from(self.pause_count)
            }
    }
}
