use super::*;

fn bundled_vad() -> StreamingSpeechVad {
    StreamingSpeechVad::new(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/vad/silero_vad.onnx"),
    )
    .expect("bundled neural VAD")
}

fn short_speech(gain: f32) -> Vec<i16> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/audio/jfk-short-speech-16k-mono.wav");
    let wav = sherpa_onnx::Wave::read(&fixture.to_string_lossy()).expect("public speech fixture");
    assert_eq!(wav.sample_rate(), 16_000);
    wav.samples()
        .iter()
        .map(|sample| (sample * gain * 32767.0) as i16)
        .collect()
}

#[test]
fn final_recording_rejects_silence_clicks_and_accidental_capture() {
    let mut vad = bundled_vad();
    assert!(!vad.analyze_recording(&[], || false).unwrap().has_speech);
    assert!(
        !vad.analyze_recording(&vec![0; 16_000], || false)
            .unwrap()
            .has_speech
    );
    let mut clicks = vec![0; 16_000];
    for index in [512, 5_000, 9_000, 13_000] {
        clicks[index] = i16::MAX;
    }
    assert!(!vad.analyze_recording(&clicks, || false).unwrap().has_speech);
    let short: Vec<i16> = (0..800)
        .map(|index| ((index as f32 * 0.085).sin() * 30_000.0) as i16)
        .collect();
    assert!(!vad.analyze_recording(&short, || false).unwrap().has_speech);
}

#[test]
fn final_recording_retains_short_quiet_speech_and_the_last_partial_window() {
    let mut vad = bundled_vad();
    for gain in [1.0, 0.1, 0.05] {
        let samples = short_speech(gain);
        assert_ne!(samples.len() % WINDOW_SAMPLES, 0);
        let decision = vad.analyze_recording(&samples, || false).unwrap();
        assert!(decision.has_speech, "short public spoken word, gain={gain}");
        assert_eq!(decision.inspected_samples, samples.len() as u64);
        assert!(decision.last_speech_sample.unwrap() <= samples.len() as u64);
    }
}

#[test]
fn cancellation_stops_between_windows_and_does_not_poison_the_next_capture() {
    let mut vad = bundled_vad();
    let checks = std::cell::Cell::new(0);
    let samples = short_speech(1.0);
    assert!(vad
        .analyze_recording(&samples, || {
            checks.set(checks.get() + 1);
            checks.get() >= 4
        })
        .is_none());
    assert!(vad.processed <= WINDOW_SAMPLES as u64 * 2);
    assert!(
        vad.analyze_recording(&samples, || false)
            .unwrap()
            .has_speech
    );
}

#[test]
fn full_packet_keeps_only_one_pending_window() {
    let mut vad = bundled_vad();
    vad.accept(&vec![0; 64_137]);
    assert_eq!(vad.processed, 64_000);
    assert_eq!(vad.pending.len(), 137);
}

#[test]
#[ignore = "requires FONO_VAD_MODEL and FONO_VAD_SPEECH_WAV (external speech fixture)"]
fn a_packet_ending_in_silence_preserves_its_earlier_speech() {
    let model = std::env::var("FONO_VAD_MODEL").expect("set Silero model path");
    let speech = std::env::var("FONO_VAD_SPEECH_WAV").expect("set 16 kHz speech WAV path");
    let wav = sherpa_onnx::Wave::read(&speech).expect("read speech WAV");
    assert_eq!(wav.sample_rate(), 16_000);
    let mut samples: Vec<i16> = wav
        .samples()
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    samples.extend(std::iter::repeat(0).take(32_000));
    let mut vad = StreamingSpeechVad::new(Path::new(&model)).unwrap();
    let whole = vad.accept(&samples);
    assert!(
        whole.has_speech,
        "must inspect speech before trailing silence"
    );
    assert!(
        !whole.current_speech,
        "two seconds of silence must finish speech"
    );
    assert!(whole.last_speech_sample.is_some());
    assert!(whole.last_speech_sample.unwrap() < whole.processed_samples - 16_000);

    vad.reset();
    let mut split = vad.accept(&[]);
    for chunk in samples.chunks(137) {
        split = vad.accept(chunk);
    }
    assert_eq!(whole.has_speech, split.has_speech);
    assert_eq!(whole.current_speech, split.current_speech);
    assert_eq!(whole.last_speech_sample, split.last_speech_sample);
    assert_eq!(whole.processed_samples, split.processed_samples);
    vad.reset();
    let silence = vad.accept(&[0; 32_000]);
    assert!(!silence.has_speech);
    assert_eq!(silence.last_speech_sample, None);
}
