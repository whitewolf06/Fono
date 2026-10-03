use serde::{Deserialize, Serialize};

use crate::BackendKind;

/// Absolute sample positions in the session's 16 kHz mono stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedSegment {
    pub text: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTranscript {
    pub text: String,
    pub segments: Vec<TimedSegment>,
    pub words: Vec<TimedSegment>,
    pub detected_language: Option<String>,
    pub transcribe_secs: f32,
    pub audio_secs: f32,
    pub backend: BackendKind,
}

/// Whisper may split Cyrillic characters across tokens. Decode only after
/// concatenation; never introduce replacement characters per token.
pub struct TimedPiece {
    pub bytes: Vec<u8>,
    pub start_sample: u64,
    pub end_sample: u64,
}

pub fn words_from_pieces(pieces: &[TimedPiece]) -> Result<Vec<TimedSegment>, std::str::Utf8Error> {
    let bytes: Vec<u8> = pieces
        .iter()
        .flat_map(|piece| piece.bytes.iter().copied())
        .collect();
    let text = std::str::from_utf8(&bytes)?;
    let mut spans = Vec::with_capacity(pieces.len());
    let mut offset = 0;
    for piece in pieces {
        spans.push((offset, offset + piece.bytes.len(), piece));
        offset += piece.bytes.len();
    }
    let mut words = Vec::new();
    let mut word_start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if character.is_whitespace() {
            if let Some(start) = word_start.take() {
                let overlapping: Vec<_> = spans
                    .iter()
                    .filter(|(begin, end, _)| *begin < index && *end > start)
                    .collect();
                if let (Some(first), Some(last)) = (overlapping.first(), overlapping.last()) {
                    words.push(TimedSegment {
                        text: text[start..index].to_owned(),
                        start_sample: first.2.start_sample,
                        end_sample: last.2.end_sample.max(first.2.start_sample),
                    });
                }
            }
        } else if word_start.is_none() {
            word_start = Some(index);
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_byte_pieces_form_whole_words_and_preserve_repeats() {
        let text = " да, да музыка";
        let pieces: Vec<_> = text
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(index, byte)| TimedPiece {
                bytes: vec![*byte],
                start_sample: 100 + index as u64,
                end_sample: 101 + index as u64,
            })
            .collect();
        let words = words_from_pieces(&pieces).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["да,", "да", "музыка"]
        );
        assert_eq!(words[0].start_sample, 101);
        assert!(words[2].end_sample > words[2].start_sample);
    }

    #[test]
    fn invalid_utf8_is_rejected_without_corrupting_hypothesis() {
        assert!(words_from_pieces(&[TimedPiece {
            bytes: vec![0xff],
            start_sample: 0,
            end_sample: 1
        }])
        .is_err());
    }
}
