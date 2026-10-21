use crate::types::DictationHistoryEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerSpeechTiming {
    pub measured_sessions: u32,
    pub unknown_sessions: u32,
    pub words: u64,
    pub speech_time_ms: u64,
    pub pause_count: u64,
    pub pause_time_ms: u64,
    pub max_pause_ms: u64,
}
impl TrainerSpeechTiming {
    pub(super) fn add(&mut self, entry: &DictationHistoryEntry, words: u32) {
        let timing = entry.metadata.as_ref().and_then(|metadata| {
            metadata.speech_timing.filter(|value| {
                value.valid()
                    && metadata.recording_duration_ms.map_or(true, |duration| {
                        value.speech_time_ms + value.pause_time_ms <= duration
                    })
            })
        });
        let Some(timing) = timing else {
            self.unknown_sessions += 1;
            return;
        };
        self.measured_sessions += 1;
        self.words += u64::from(words);
        self.speech_time_ms += timing.speech_time_ms;
        self.pause_count += u64::from(timing.pause_count);
        self.pause_time_ms += timing.pause_time_ms;
        self.max_pause_ms = self.max_pause_ms.max(timing.max_pause_ms);
    }
}
