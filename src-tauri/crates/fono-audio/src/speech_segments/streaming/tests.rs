use super::*;

fn model() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/vad/silero_vad.onnx")
}

fn speech(relative: &str, gain: f32) -> Vec<i16> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(relative);
    let wav = sherpa_onnx::Wave::read(&path.to_string_lossy()).unwrap();
    assert_eq!(wav.sample_rate(), SAMPLE_RATE as i32);
    wav.samples()
        .iter()
        .map(|sample| (sample * gain * 32767.0) as i16)
        .collect()
}

fn collect(samples: &[i16], maximum: usize, packets: &[usize]) -> Vec<ClosedSpeechSegment> {
    let mut stream = StreamingSpeechSegmenter::new(&model(), maximum).unwrap();
    let mut output = Vec::new();
    let mut offset = 0;
    let mut packet = 0;
    while offset < samples.len() {
        stream
            .accept(&[], |_| panic!("empty input cannot emit"))
            .unwrap();
        let end = (offset + packets[packet % packets.len()]).min(samples.len());
        stream
            .accept(&samples[offset..end], |segment| output.push(segment))
            .unwrap();
        assert_eq!(stream.received_samples(), end);
        assert!(stream.buffered_samples() < maximum + WINDOW);
        offset = end;
        packet += 1;
    }
    stream.finish(|segment| output.push(segment)).unwrap();
    assert_exact_partition(samples, &output, maximum);
    output
}

fn assert_exact_partition(samples: &[i16], output: &[ClosedSpeechSegment], maximum: usize) {
    let mut next = 0;
    for segment in output {
        assert_eq!(segment.range.start, next);
        assert!(segment.range.end > next);
        assert!(segment.samples.len() <= maximum);
        assert_eq!(segment.samples, samples[next..segment.range.end]);
        next = segment.range.end;
    }
    assert_eq!(next, samples.len());
}

#[test]
fn neural_boundaries_ignore_packet_sizes_and_preserve_short_quiet_phrases_and_tail() {
    let mut samples = speech("stt-quality/ru-01.wav", 1.0);
    samples.extend(vec![0; SAMPLE_RATE]);
    samples.extend(speech("audio/jfk-short-speech-16k-mono.wav", 0.05));
    samples.extend(vec![0; SAMPLE_RATE]);
    samples.extend((0..137).map(|index| index * 7));
    let maximum = super::super::DEFAULT_MAX_SEGMENT_SAMPLES;
    let whole = collect(&samples, maximum, &[samples.len()]);
    assert!(whole
        .iter()
        .any(|segment| !segment.range.forced && segment.range.end < samples.len()));
    for packets in [&[1][..], &[137], &[512], &[4097], &[11, 777, 3, 2048, 513]] {
        assert_eq!(
            collect(&samples, maximum, packets),
            whole,
            "packets={packets:?}"
        );
    }
    // Independent admission confirms that the quiet one-word fixture remains
    // speech, rather than proving PCM equality for an already rejected word.
    let mut vad = StreamingSpeechVad::new(&model()).unwrap();
    let quiet = speech("audio/jfk-short-speech-16k-mono.wav", 0.05);
    assert!(vad.analyze_recording(&quiet, || false).unwrap().has_speech);
}

#[test]
fn pause_confirmation_waits_six_hundred_ms_and_forced_vad_closes_are_not_pauses() {
    let end = 2 * SAMPLE_RATE;
    assert_eq!(
        confirmed_pause_end(false, Some(end), end + CAPTURE_PAUSE - 1, CAPTURE_PAUSE),
        None
    );
    assert_eq!(
        confirmed_pause_end(false, Some(end), end + CAPTURE_PAUSE, CAPTURE_PAUSE),
        Some(end)
    );
    assert_eq!(
        confirmed_pause_end(true, Some(end), end + SAMPLE_RATE, CAPTURE_PAUSE),
        None
    );

    let utterance = speech("audio/jfk-short-speech-16k-mono.wav", 1.0);
    let mut stream = StreamingSpeechSegmenter::new(&model(), 25 * SAMPLE_RATE).unwrap();
    stream
        .accept(&utterance, |_| {
            panic!("a short open utterance cannot reach a cap")
        })
        .unwrap();
    let mut emitted_at = Vec::new();
    for _ in 0..(2 * SAMPLE_RATE / WINDOW) {
        let inspected = stream.received_samples() + WINDOW;
        stream
            .accept(&[0; WINDOW], |segment| {
                emitted_at.push((inspected, segment))
            })
            .unwrap();
    }
    assert_eq!(emitted_at.len(), 1);
    let (inspected, segment) = &emitted_at[0];
    assert!(!segment.range.forced);
    let neural_end = segment.range.end - super::super::PAUSE_PADDING;
    assert!(*inspected >= neural_end + CAPTURE_PAUSE);
    assert!(*inspected - neural_end <= CAPTURE_PAUSE + WINDOW);
}

#[test]
fn a_large_packet_and_long_silence_do_not_grow_or_queue_retained_pcm() {
    let maximum = 1025;
    let mut stream = StreamingSpeechSegmenter::new(&model(), maximum).unwrap();
    let capacity = stream.pending.capacity();
    let packet = vec![0; maximum * 12 + 137];
    let mut emitted = 0;
    for _ in 0..30 {
        stream
            .accept(&packet, |segment| {
                assert_eq!(segment.samples.len(), maximum);
                assert!(segment.range.forced);
                emitted += segment.samples.len();
            })
            .unwrap();
        assert!(stream.buffered_samples() < maximum + WINDOW);
        assert_eq!(
            stream.pending.capacity(),
            capacity,
            "no growth after construction"
        );
    }
    let received = stream.received_samples();
    stream
        .finish(|segment| {
            assert!(segment.samples.len() <= maximum);
            emitted += segment.samples.len();
        })
        .unwrap();
    assert_eq!(emitted, received);
}

#[test]
fn stop_preserves_partial_windows_and_non_aligned_caps_without_duplicate_samples() {
    for length in [0, 1, 137, 511, 512, 513, 1025, 1026, 1547, 4097] {
        let samples: Vec<i16> = (0..length).map(|index| (index % 257) as i16).collect();
        let whole = collect(&samples, 1025, &[length.max(1)]);
        assert_eq!(collect(&samples, 1025, &[137, 512, 1]), whole);
        assert_eq!(
            whole.last().map(|segment| segment.range.end).unwrap_or(0),
            length
        );
    }
}

#[test]
fn invalid_caps_and_clock_overflow_fail_before_accepting_pcm() {
    for maximum in [0, 511, 30 * SAMPLE_RATE + 1, usize::MAX] {
        assert!(StreamingSpeechSegmenter::new(&model(), maximum).is_err());
    }
    let mut stream = StreamingSpeechSegmenter::new(&model(), 1025).unwrap();
    stream.received = usize::MAX;
    assert!(stream
        .accept(&[1], |_| panic!("overflow cannot emit"))
        .is_err());
    assert_eq!(stream.buffered_samples(), 0);
}
