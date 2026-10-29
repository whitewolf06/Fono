//! Request-owned options for the server's rich recognition endpoints.

use fono_stt_protocol::DecodingOptions;
use fono_types::{SubtitleSegment, TranscriptionResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimestampMode {
    #[default]
    None,
    Segments,
    Words,
}

#[derive(Debug, Clone, Default)]
pub struct RecognitionOptions {
    pub decoding: DecodingOptions,
    pub glossary: Vec<String>,
    pub timestamps: TimestampMode,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecognitionOutput {
    pub result: TranscriptionResult,
    pub words: Vec<SubtitleSegment>,
}
