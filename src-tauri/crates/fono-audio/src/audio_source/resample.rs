/// Streaming rate conversion with an anti-alias low-pass before downsampling.
///
/// The FIR state, fractional position and boundary sample survive every callback;
/// packet boundaries cannot change the waveform. Coefficients and history are
/// allocated once, in `new`. Callers reserve `output_capacity` before capture.
pub struct StreamingResampler {
    input_rate: u64,
    output_rate: u64,
    position: u64,
    previous: Option<i16>,
    low_pass: Option<LowPass>,
    input_samples: u64,
    output_samples: u64,
    finished: bool,
}

struct LowPass {
    coefficients: Box<[f64]>,
    history: Box<[f64]>,
    cursor: usize,
}

impl LowPass {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        // A Blackman-windowed sinc: cutoff 90% of the output Nyquist, with
        // approximately 1.5 ms group delay for usual 44.1/48/96/192 kHz inputs.
        // Bound allocation even if this public constructor receives unusual rates.
        let half =
            ((24 * u64::from(input_rate)).div_ceil(u64::from(output_rate))).clamp(32, 512) as usize;
        let length = 2 * half + 1;
        let cutoff = 0.45 * f64::from(output_rate) / f64::from(input_rate);
        let mut coefficients = vec![0.0; length];
        for (index, coefficient) in coefficients.iter_mut().enumerate() {
            let distance = index as f64 - half as f64;
            let sinc = if distance == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * distance).sin()
                    / (std::f64::consts::PI * distance)
            };
            let phase = 2.0 * std::f64::consts::PI * index as f64 / (length - 1) as f64;
            *coefficient = sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos());
        }
        let gain: f64 = coefficients.iter().sum();
        for coefficient in &mut coefficients {
            *coefficient /= gain;
        }
        Self {
            coefficients: coefficients.into_boxed_slice(),
            history: vec![0.0; length].into_boxed_slice(),
            cursor: 0,
        }
    }

    fn push(&mut self, sample: i16) -> i16 {
        self.history[self.cursor] = f64::from(sample);
        // Split at the cursor instead of doing modulo for every tap.
        let mut value = 0.0;
        for (coefficient, sample) in self.coefficients[..=self.cursor]
            .iter()
            .zip(self.history[..=self.cursor].iter().rev())
        {
            value += coefficient * sample;
        }
        for (coefficient, sample) in self.coefficients[self.cursor + 1..]
            .iter()
            .zip(self.history[self.cursor + 1..].iter().rev())
        {
            value += coefficient * sample;
        }
        self.cursor += 1;
        if self.cursor == self.history.len() {
            self.cursor = 0;
        }
        value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
    }
}

impl StreamingResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        assert!(input_rate > 0 && output_rate > 0);
        let low_pass = (input_rate > output_rate).then(|| LowPass::new(input_rate, output_rate));
        // Wait for the filter's lookahead instead of inserting artificial leading
        // silence into the sample clock. The first emitted frame represents t=0.
        let position = low_pass.as_ref().map_or(0, |filter| {
            (filter.coefficients.len() / 2) as u64 * u64::from(output_rate)
        });
        Self {
            input_rate: input_rate as u64,
            output_rate: output_rate as u64,
            position,
            previous: None,
            low_pass,
            input_samples: 0,
            output_samples: 0,
            finished: false,
        }
    }

    /// Enough reusable storage for any packet up to this many samples, or finish.
    pub fn output_capacity(&self, input_samples: usize) -> usize {
        let samples = input_samples.max(self.delay_input_samples() + 2);
        ((samples as u128 * u128::from(self.output_rate)).div_ceil(u128::from(self.input_rate)) + 2)
            .min(usize::MAX as u128) as usize
    }

    /// Low-pass group delay in input frames (zero for bypass/upsampling).
    pub fn delay_input_samples(&self) -> usize {
        self.low_pass
            .as_ref()
            .map_or(0, |filter| filter.coefficients.len() / 2)
    }

    pub fn process(&mut self, input: &[i16], output: &mut Vec<i16>) {
        if self.finished {
            output.clear();
            return;
        }
        self.input_samples = self.input_samples.saturating_add(input.len() as u64);
        self.process_samples(input, output);
        self.output_samples = self.output_samples.saturating_add(output.len() as u64);
    }

    /// Complete a finite source with zero extension, keeping the pre-filter
    /// sample count. Idempotent; construct a new resampler for a new source.
    /// Continuous microphone streams simply keep supplying subsequent packets.
    pub fn finish(&mut self, output: &mut Vec<i16>) {
        output.clear();
        if self.finished {
            return;
        }
        self.finished = true;
        if self.low_pass.is_none() || self.input_samples == 0 {
            return;
        }
        let expected = (u128::from(self.input_samples.saturating_sub(1))
            * u128::from(self.output_rate))
        .div_ceil(u128::from(self.input_rate)) as u64;
        let remaining = expected.saturating_sub(self.output_samples) as usize;
        let zeros = [0; 514];
        let length = self.delay_input_samples() + 2;
        self.process_samples(&zeros[..length], output);
        output.truncate(remaining);
        self.output_samples = self.output_samples.saturating_add(output.len() as u64);
    }

    fn process_samples(&mut self, input: &[i16], output: &mut Vec<i16>) {
        output.clear();
        if input.is_empty() {
            return;
        }
        if self.input_rate == self.output_rate {
            output.extend_from_slice(input);
            return;
        }
        if let Some(filter) = &mut self.low_pass {
            for &sample in input {
                let current = filter.push(sample);
                if let Some(previous) = self.previous {
                    // One input interval at a time: no filtered packet buffer,
                    // and the exact same operation order for every packet split.
                    while self.position < self.output_rate {
                        let fraction = self.position as f64 / self.output_rate as f64;
                        output.push(
                            (f64::from(previous)
                                + (f64::from(current) - f64::from(previous)) * fraction)
                                .round() as i16,
                        );
                        self.position += self.input_rate;
                    }
                    self.position -= self.output_rate;
                }
                self.previous = Some(current);
            }
            return;
        }
        // With a previous sample, logical input starts one sample earlier.
        let offset = usize::from(self.previous.is_some());
        let length = input.len() + offset;
        let boundary = (length - 1) as u64 * self.output_rate;
        while self.position < boundary {
            let index = (self.position / self.output_rate) as usize;
            let sample = |i: usize| {
                if offset == 1 && i == 0 {
                    self.previous.unwrap()
                } else {
                    input[i - offset]
                }
            };
            let a = sample(index) as f64;
            let b = sample(index + 1) as f64;
            let fraction = (self.position % self.output_rate) as f64 / self.output_rate as f64;
            output.push((a + (b - a) * fraction).round() as i16);
            self.position += self.input_rate;
        }
        self.position -= boundary;
        self.previous = input.last().copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, frequency: f64) -> Vec<i16> {
        (0..rate)
            .map(|index| {
                (24_000.0
                    * (2.0 * std::f64::consts::PI * frequency * index as f64 / rate as f64).sin())
                .round() as i16
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f64 {
        (samples
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>()
            / samples.len() as f64)
            .sqrt()
    }

    fn gain_db(input_rate: u32, output_rate: u32, frequency: f64) -> f64 {
        let input = tone(input_rate, frequency);
        let mut resampler = StreamingResampler::new(input_rate, output_rate);
        let mut output = Vec::with_capacity(resampler.output_capacity(input.len()));
        resampler.process(&input, &mut output);
        // Discard startup, then measure a steady window (including quantization).
        let amplitude = rms(&output[output_rate as usize / 10..]);
        20.0 * (amplitude / rms(&input[input_rate as usize / 10..])).log10()
    }

    #[test]
    fn packet_boundaries_do_not_change_output() {
        for (input_rate, output_rate) in [
            (44_100, 16_000),
            (48_000, 16_000),
            (16_000, 8_000),
            (8_000, 16_000),
            (96_000, 16_000),
            (192_000, 16_000),
            (16_000, 16_000),
        ] {
            let input: Vec<i16> = (0..10001)
                .map(|i| ((i * 127) % 20000 - 10000) as i16)
                .collect();
            let mut whole = StreamingResampler::new(input_rate, output_rate);
            let mut expected = Vec::new();
            whole.process(&input, &mut expected);
            for packet_size in [1, 2, 3, 137, 512, 4096] {
                let mut split = StreamingResampler::new(input_rate, output_rate);
                let mut actual = Vec::new();
                let mut chunk_output = Vec::new();
                for chunk in input.chunks(packet_size) {
                    split.process(&[], &mut chunk_output);
                    assert!(chunk_output.is_empty());
                    split.process(chunk, &mut chunk_output);
                    actual.extend_from_slice(&chunk_output);
                }
                assert_eq!(
                    expected, actual,
                    "{input_rate}/{output_rate}, {packet_size}"
                );
            }
        }
    }

    #[test]
    fn rejects_out_of_band_audio_before_decimation() {
        for (input_rate, output_rate, frequency) in [
            (48_000, 16_000, 12_000.0),
            (44_100, 16_000, 12_000.0),
            (96_000, 16_000, 12_000.0),
            (192_000, 16_000, 12_000.0),
            (16_000, 8_000, 6_000.0),
        ] {
            let gain = gain_db(input_rate, output_rate, frequency);
            eprintln!("stopband {input_rate}/{output_rate} at {frequency} Hz: {gain:.2} dB");
            assert!(gain <= -40.0, "stopband gain: {gain:.2} dB");
        }
    }

    #[test]
    fn preserves_speech_passband() {
        for input_rate in [44_100, 48_000, 96_000, 192_000] {
            for frequency in [100.0, 1_000.0, 3_000.0, 5_000.0] {
                let gain = gain_db(input_rate, 16_000, frequency);
                assert!(
                    gain.abs() < 0.6,
                    "{input_rate}/16000 at {frequency} Hz: {gain:.2} dB"
                );
            }
        }
    }

    #[test]
    fn latency_is_bounded_and_output_does_not_wait_for_a_full_packet() {
        for input_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut preview = StreamingResampler::new(input_rate, 16_000);
            let mut first = Vec::with_capacity(preview.output_capacity(1));
            let first_frame = (0..input_rate)
                .find(|_| {
                    preview.process(&[1_000], &mut first);
                    !first.is_empty()
                })
                .unwrap() as usize;
            assert_eq!(first_frame, preview.delay_input_samples() + 1);
            let mut resampler = StreamingResampler::new(input_rate, 16_000);
            let delay_ms = 1000.0 * resampler.delay_input_samples() as f64 / input_rate as f64;
            assert!(delay_ms <= 1.6, "{input_rate}: {delay_ms} ms");
            let mut input = vec![0; input_rate as usize / 10];
            input[0] = 30_000;
            let mut output = Vec::new();
            resampler.process(&input, &mut output);
            let peak = output
                .iter()
                .enumerate()
                .max_by_key(|(_, sample)| sample.abs())
                .unwrap()
                .0;
            assert_eq!(peak, 0, "group delay must not become a zero prefix");
            let mut tail = Vec::new();
            resampler.finish(&mut tail);
            assert_eq!(output.len() + tail.len(), 1_600);
        }
    }

    #[test]
    fn finishing_preserves_the_tail_and_is_independent_of_packets() {
        let mut input = vec![0; 4_800];
        input[4_796] = 30_000;
        let mut expected = Vec::new();
        let mut whole = StreamingResampler::new(48_000, 16_000);
        whole.process(&input, &mut expected);
        let mut tail = Vec::new();
        whole.finish(&mut tail);
        expected.extend_from_slice(&tail);
        assert_eq!(expected.len(), 1_600);
        assert!(expected[1_599].abs() > 5_000);
        whole.finish(&mut tail);
        assert!(tail.is_empty());
        whole.process(&input, &mut tail);
        assert!(tail.is_empty());
        for packet in [1, 137, 512] {
            let mut split = StreamingResampler::new(48_000, 16_000);
            let mut actual = Vec::new();
            for chunk in input.chunks(packet) {
                split.process(chunk, &mut tail);
                actual.extend_from_slice(&tail);
            }
            split.finish(&mut tail);
            actual.extend_from_slice(&tail);
            assert_eq!(expected, actual);
        }
        for length in [0, 1, 2, 5, 20, 71, 72, 73, 137] {
            let mut short = StreamingResampler::new(48_000, 16_000);
            short.process(&input[..length], &mut tail);
            let count = tail.len();
            short.finish(&mut tail);
            assert_eq!(count + tail.len(), length.saturating_sub(1).div_ceil(3));
        }
    }

    #[test]
    fn dc_and_same_rate_are_preserved() {
        let input = vec![12_345; 10_000];
        for (input_rate, output_rate) in [(48_000, 16_000), (44_100, 16_000), (16_000, 16_000)] {
            let mut resampler = StreamingResampler::new(input_rate, output_rate);
            let mut output = Vec::new();
            resampler.process(&input, &mut output);
            assert!(output[500..].iter().all(|&sample| sample == 12_345));
            if input_rate == output_rate {
                assert_eq!(input, output);
                assert_eq!(resampler.delay_input_samples(), 0);
            }
        }
    }
}
