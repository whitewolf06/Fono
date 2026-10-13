use serde::{Deserialize, Serialize};

use crate::types::{SpeechFindingKind, SpeechLlmDataScope};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerCounts {
    pub filler: u32,
    pub repetition: u32,
    pub self_correction: u32,
    pub unfinished: u32,
}

impl TrainerCounts {
    pub fn total(&self) -> u32 {
        self.filler + self.repetition + self.self_correction + self.unfinished
    }

    pub fn add(&mut self, other: &Self) {
        self.filler += other.filler;
        self.repetition += other.repetition;
        self.self_correction += other.self_correction;
        self.unfinished += other.unfinished;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerExample {
    pub session_id: String,
    pub fragment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerTerm {
    pub key: String,
    pub label: String,
    pub kind: SpeechFindingKind,
    pub count: u32,
    pub density: f32,
    pub affected_sessions: u32,
    pub session_ids: Vec<String>,
    pub examples: Vec<TrainerExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerDay {
    pub date: String,
    pub sessions: u32,
    pub words: u32,
    pub counts: TrainerCounts,
    pub density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerSnapshot {
    pub fingerprint: String,
    pub rules_version: String,
    pub days: u32,
    pub from: String,
    pub to: String,
    pub generated_at: String,
    pub included_sessions: u32,
    pub excluded_sessions: u32,
    pub pending_sessions: u32,
    pub unavailable_sessions: u32,
    pub total_words: u32,
    pub counts: TrainerCounts,
    pub density: f32,
    pub terms: Vec<TrainerTerm>,
    pub daily: Vec<TrainerDay>,
    pub session_ids: Vec<String>,
    pub insufficient_data: bool,
    pub history_limit: u32,
    pub coverage_start: Option<String>,
    pub retention_days: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiPriority {
    pub title: String,
    pub basis: String,
    pub exercise: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiContextLimits {
    pub max_shared_sessions: usize,
    pub max_characters_per_session: usize,
    pub max_patterns: usize,
    pub max_examples_per_pattern: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiContext {
    pub data_scope: SpeechLlmDataScope,
    pub payload_bytes: usize,
    pub payload_byte_limit: usize,
    pub reduced: bool,
    pub metrics_sessions: usize,
    pub metrics_words: usize,
    pub available_sessions: usize,
    pub shared_sessions: usize,
    pub omitted_sessions: usize,
    pub shortened_sessions: usize,
    pub available_words: usize,
    pub shared_words: usize,
    pub omitted_words: usize,
    pub available_characters: usize,
    pub shared_characters: usize,
    pub omitted_characters: usize,
    pub patterns_available: usize,
    pub patterns_shared: usize,
    pub examples_available: usize,
    pub examples_shared: usize,
    pub limits: TrainerAiContextLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiContextPreflight {
    pub metrics_only: TrainerAiContext,
    pub configured_shared: Option<TrainerAiContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiSummary {
    pub fingerprint: String,
    pub snapshot_fingerprint: String,
    pub generated_at: String,
    pub profile_id: String,
    pub profile_name: String,
    pub model: String,
    pub data_scope: SpeechLlmDataScope,
    #[serde(default)]
    pub context: Option<TrainerAiContext>,
    pub summary: String,
    pub priorities: Vec<TrainerAiPriority>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerAiConfiguration {
    pub enabled: bool,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub model: Option<String>,
    pub cloud: bool,
    pub cloud_consent: bool,
    pub data_scope: SpeechLlmDataScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerReport {
    pub snapshot: TrainerSnapshot,
    pub current_fingerprint: String,
    pub stale: bool,
    pub ai_fingerprint: String,
    pub ai_stale: bool,
    pub ai_summary: Option<TrainerAiSummary>,
    pub ai_configuration: TrainerAiConfiguration,
    pub ai_disabled_reason: Option<String>,
    #[serde(default)]
    pub ai_context: Option<TrainerAiContextPreflight>,
}

#[derive(Debug, Clone)]
pub struct TrainerSnapshotState {
    pub snapshot: TrainerSnapshot,
    pub current_fingerprint: String,
    pub stale: bool,
    pub ai_summary: Option<TrainerAiSummary>,
}
