//! Local export from recognizer timing. No evenly distributed/fake timestamps.
use super::transcription_contract::{SubtitleSegment, TranscriptionResult};
use crate::{
    error::{AppError, AppResult},
    stt::TimedSegment,
};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Txt,
    Srt,
    Vtt,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Srt => "srt",
            Self::Vtt => "vtt",
        }
    }
}

/// Group actual word timestamps into readable cues, retaining silent gaps.
pub fn cues(words: &[TimedSegment], audio_samples: u64) -> Vec<SubtitleSegment> {
    let mut cues: Vec<SubtitleSegment> = Vec::new();
    for word in words {
        let text = word.text.trim();
        let start_ms = word.start_sample.min(audio_samples) / 16;
        let end_ms = word.end_sample.min(audio_samples) / 16;
        if text.is_empty() || end_ms <= start_ms {
            continue;
        }
        let append = cues.last().is_some_and(|cue| {
            start_ms >= cue.start_ms
                && start_ms <= cue.end_ms.saturating_add(800)
                && end_ms.saturating_sub(cue.start_ms) <= 6000
                && cue.text.chars().count() + text.chars().count() < 64
                && !cue.text.ends_with(['.', '!', '?'])
        });
        if append {
            let cue = cues.last_mut().unwrap();
            if !text.starts_with(['.', ',', '!', '?', ':', ';']) {
                cue.text.push(' ');
            }
            cue.text.push_str(text);
            cue.end_ms = cue.end_ms.max(end_ms);
        } else {
            let start_ms = start_ms.max(cues.last().map(|c| c.end_ms).unwrap_or(0));
            if end_ms > start_ms {
                cues.push(SubtitleSegment {
                    start_ms,
                    end_ms,
                    text: text.into(),
                });
            }
        }
    }
    cues
}

pub fn render(result: &TranscriptionResult, format: ExportFormat) -> AppResult<String> {
    if matches!(format, ExportFormat::Txt) {
        return Ok(format!("{}\n", result.text.trim()));
    }
    if result.segments.is_empty() {
        return Err(AppError::Config("В этом результате нет временных меток. Сохраните TXT или распознайте файл моделью с поддержкой меток.".into()));
    }
    let mut output = if matches!(format, ExportFormat::Vtt) {
        "WEBVTT\n\n".into()
    } else {
        String::new()
    };
    let mut previous_end = 0;
    for (index, cue) in result.segments.iter().enumerate() {
        if cue.start_ms < previous_end || cue.end_ms <= cue.start_ms {
            return Err(AppError::Config(
                "Временные метки результата повреждены. Сохраните TXT.".into(),
            ));
        }
        previous_end = cue.end_ms;
        let separator = if matches!(format, ExportFormat::Srt) {
            ','
        } else {
            '.'
        };
        let text = cue.text.split_whitespace().collect::<Vec<_>>().join(" ");
        // Subtitle markup is plain transcript text, never executable cue markup.
        let text = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            timestamp(cue.start_ms, separator),
            timestamp(cue.end_ms, separator),
            text
        ));
    }
    Ok(output)
}

fn timestamp(ms: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result() -> TranscriptionResult {
        serde_json::from_value(serde_json::json!({"protocol_version":1,"text":"Привет","model":"base","detected_language":null,"audio_seconds":1800.0,"transcribe_seconds":1.0,"backend":"CPU"})).unwrap()
    }
    #[test]
    fn thirty_minute_timeline_preserves_silence_and_millisecond_precision() {
        let mut result = result();
        result.segments = cues(
            &[
                TimedSegment {
                    text: "Начало.".into(),
                    start_sample: 16_000,
                    end_sample: 32_000,
                },
                TimedSegment {
                    text: "Конец.".into(),
                    start_sample: 1_799_123 * 16,
                    end_sample: 1_799_987 * 16,
                },
            ],
            1800 * 16_000,
        );
        let srt = render(&result, ExportFormat::Srt).unwrap();
        assert!(srt.contains("00:00:01,000 --> 00:00:02,000"));
        assert!(srt.contains("00:29:59,123 --> 00:29:59,987"));
        assert!(render(&result, ExportFormat::Vtt)
            .unwrap()
            .starts_with("WEBVTT\n\n1\n00:00:01.000"));
    }
    #[test]
    fn old_results_are_txt_only_and_invalid_cues_are_rejected() {
        assert_eq!(timestamp(3_600_001, '.'), "01:00:00.001");
        let mut result = result();
        assert_eq!(render(&result, ExportFormat::Txt).unwrap(), "Привет\n");
        assert!(render(&result, ExportFormat::Srt).is_err());
        result.segments = vec![SubtitleSegment {
            start_ms: 20,
            end_ms: 10,
            text: "bad".into(),
        }];
        assert!(render(&result, ExportFormat::Vtt).is_err());
    }
}
