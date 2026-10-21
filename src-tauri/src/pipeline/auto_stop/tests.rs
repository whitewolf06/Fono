use super::*;

fn decision(processed: u64, last_speech: u64, speaking: bool) -> SpeechDecision {
    SpeechDecision {
        processed,
        last_speech,
        speaking,
        has_speech: last_speech > 0,
    }
}

#[test]
fn legacy_default_is_off_and_timeout_validation_is_bounded() {
    let mut settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.toggle_auto_stop_ms, 0);
    for value in [0, 2_000, 2_500, 10_000] {
        settings.toggle_auto_stop_ms = value;
        assert!(settings.validate_toggle_auto_stop().is_ok());
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(restored.toggle_auto_stop_ms, value);
    }
    for value in [1, 1_999, 10_001, u64::MAX] {
        settings.toggle_auto_stop_ms = value;
        assert!(settings.validate_toggle_auto_stop().is_err());
    }
}

#[test]
fn only_ordinary_standard_toggle_capture_arms_auto_stop() {
    let mut settings = Settings {
        hotkey_mode: HotkeyMode::Toggle,
        toggle_auto_stop_ms: 3_000,
        ..Settings::default()
    };
    for source in [OperationSource::Ui, OperationSource::Hotkey] {
        assert_eq!(timeout(&settings, source, false), Some(3_000));
        assert_eq!(timeout(&settings, source, true), None);
    }
    for source in [
        OperationSource::WakeWord,
        OperationSource::Diagnostics,
        OperationSource::Service,
    ] {
        assert_eq!(timeout(&settings, source, false), None);
    }
    settings.hotkey_mode = HotkeyMode::Hold;
    assert_eq!(timeout(&settings, OperationSource::Ui, false), None);
    settings.hotkey_mode = HotkeyMode::Toggle;
    settings.toggle_auto_stop_ms = 0;
    assert_eq!(timeout(&settings, OperationSource::Hotkey, false), None);
}

#[test]
fn silence_uses_only_processed_audio_and_speech_resets_the_deadline() {
    let mut clock = SilenceClock::new(17, 2_000);
    assert_eq!(clock.observe(&decision(0, 0, false)).remaining_ms, 2_000);
    assert_eq!(
        clock.observe(&decision(31_999, 0, false)).phase,
        Phase::Silence
    );
    assert_eq!(
        clock.observe(&decision(32_000, 0, false)).phase,
        Phase::Stopping
    );
    let mut clock = SilenceClock::new(18, 10_000);
    let speech = clock.observe(&decision(16_000, 16_000, true));
    assert_eq!(speech.phase, Phase::Listening);
    assert_eq!(speech.remaining_ms, 10_000);
    assert_eq!(
        clock
            .observe(&decision(175_488, 16_000, false))
            .remaining_ms,
        32
    );
    assert_eq!(
        clock
            .observe(&decision(176_000, 176_000, true))
            .remaining_ms,
        10_000
    );
    assert_eq!(
        clock.observe(&decision(336_000, 176_000, false)).phase,
        Phase::Stopping
    );
}

#[cfg(feature = "speech-vad")]
#[test]
fn real_vad_replay_handles_short_quiet_speech_and_initial_silence() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let wav =
        std::fs::read(root.join("tests/fixtures/audio/jfk-short-speech-16k-mono.wav")).unwrap();
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    let speech: Vec<i16> = wav[44..]
        .chunks_exact(2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]))
        .collect();
    for gain in [1.0, 0.12, 0.0] {
        let mut vad =
            fono_audio::StreamingSpeechVad::new(&root.join("resources/vad/silero_vad.onnx"))
                .unwrap();
        let mut clock = SilenceClock::new(3, 2_000);
        let mut samples = vec![0; 1_600];
        samples.extend(speech.iter().map(|s| (*s as f32 * gain) as i16));
        samples.resize(samples.len() + 64_000, 0);
        let mut heard = false;
        let mut stop_at = None;
        for window in samples.chunks(512) {
            let neural = vad.accept(window);
            heard |= neural.has_speech;
            let status = clock.observe(&decision(
                neural.processed_samples,
                neural.last_speech_sample.unwrap_or(0),
                neural.current_speech,
            ));
            if status.phase == Phase::Stopping {
                stop_at = Some(neural.processed_samples);
                break;
            }
        }
        assert_eq!(heard, gain > 0.0, "gain={gain}");
        assert!(stop_at.is_some());
        if gain == 0.0 {
            assert_eq!(stop_at, Some(32_256));
        } else {
            assert!(stop_at.unwrap() > 32_256);
        }
    }
}
