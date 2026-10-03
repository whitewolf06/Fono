//! Keep an utterance intact while it is being spoken. Retiring PCM after every
//! accepted word makes Whisper decode cut syllables and shifts its token times.
pub const OVERLAP: u64 = 2 * 16_000;

pub fn next_start(
    current: u64,
    committed_end: u64,
    closed_speech_end: Option<u64>,
    window_end: u64,
    bounded: bool,
) -> Option<u64> {
    if let Some(end) = closed_speech_end {
        let next = current.max(end);
        if next > current || !bounded {
            return Some(next);
        }
        // This speech endpoint was already retired. A full window of trailing
        // silence must advance; otherwise Finish keeps decoding it forever.
        // Retain the fresh two-second edge for unconfirmed syllables/VAD input.
        let next = current.max(window_end.saturating_sub(OVERLAP));
        return (next > current).then_some(next);
    }
    if bounded {
        let next = current.max(committed_end.saturating_sub(OVERLAP));
        return (next > current).then_some(next);
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_partial_commit_does_not_cut_a_live_utterance() {
        assert_eq!(next_start(0, 8 * 16_000, None, 10 * 16_000, false), Some(0));
        assert_eq!(
            next_start(0, 8 * 16_000, Some(9 * 16_000), 10 * 16_000, false),
            Some(9 * 16_000)
        );
    }
    #[test]
    fn a_long_utterance_retains_two_seconds_of_overlap() {
        assert_eq!(
            next_start(0, 19 * 16_000, None, 20 * 16_000, true),
            Some(17 * 16_000)
        );
        assert_eq!(
            next_start(17 * 16_000, 18 * 16_000, None, 37 * 16_000, true),
            None
        );
    }
    #[test]
    fn first_quiet_endpoint_retires_only_confirmed_speech() {
        assert_eq!(
            next_start(0, 9 * 16_000, Some(10 * 16_000), 20 * 16_000, true),
            Some(10 * 16_000)
        );
    }
    fn drain_quiet_tail(seconds: u64) -> Vec<u64> {
        let speech_end = 10 * 16_000;
        let audio_end = speech_end + seconds * 16_000;
        let mut current = speech_end;
        let mut windows = Vec::new();
        loop {
            let end = (current + 20 * 16_000).min(audio_end);
            windows.push(current);
            if end - current < 20 * 16_000 {
                // After Finish the remaining short window is finalized once.
                assert_eq!(end, audio_end);
                break;
            }
            let next = next_start(current, speech_end, Some(speech_end), end, true).unwrap();
            assert!(next > current);
            assert_eq!(next, end - OVERLAP);
            current = next;
            assert!(windows.len() <= 3, "quiet windows must make progress");
        }
        windows
    }
    #[test]
    fn twenty_seconds_of_trailing_silence_reach_the_final_window() {
        assert_eq!(drain_quiet_tail(20), vec![10 * 16_000, 28 * 16_000]);
    }
    #[test]
    fn forty_seconds_of_trailing_silence_reach_the_final_window() {
        assert_eq!(
            drain_quiet_tail(40),
            vec![10 * 16_000, 28 * 16_000, 46 * 16_000]
        );
    }
}
