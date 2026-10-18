/// Streaming rate conversion with a band-limited FIR for downsampling.
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
    half: usize,
    phase_resolution: u64,
}

impl LowPass {
    fn new(input_rate: u32, output_rate: u32) -> Self {
        // The passband/stopband contract at 16 kHz is <= 0.1 dB through
        // 7 kHz and >= 60 dB rejection from 8 kHz. The transition is centered
        // at 7.5 kHz; the old 7.2 kHz cutoff removed useful in-band energy.
        // 48 output frames of lookahead are <= 3 ms at 44.1/48/96/192 kHz.
        // Extremely unusual ratios retain bounded storage rather than claiming
        // the same transition width with an arbitrarily short input kernel.
        let half = ((48 * u64::from(input_rate)) / u64::from(output_rate)).clamp(32, 1024) as usize;
        let length = 2 * half + 1;
        let cutoff = (15.0 / 32.0) * f64::from(output_rate) / f64::from(input_rate);
        let mut divisor = input_rate;
        let mut remainder = output_rate;
        while remainder != 0 {
            (divisor, remainder) = (remainder, divisor % remainder);
        }
        // Common rational clocks use their exact phases (160 for 44.1 -> 16).
        // Other clocks interpolate coefficients, not quantized PCM, between
        // at most 257 kernels. Even arbitrary public input rates cannot cause
        // unbounded constructor allocation. Nothing is allocated while streaming.
        let phase_resolution = u64::from(output_rate / divisor).min(256);
        let mut coefficients = vec![0.0; (phase_resolution as usize + 1) * length];
        for (phase, kernel) in coefficients.chunks_exact_mut(length).enumerate() {
            let fraction = phase as f64 / phase_resolution as f64;
            for (index, coefficient) in kernel.iter_mut().enumerate() {
                let distance = index as f64 - half as f64 - fraction;
                if distance.abs() > half as f64 {
                    continue;
                }
                let sinc = if distance == 0.0 {
                    2.0 * cutoff
                } else {
                    (2.0 * std::f64::consts::PI * cutoff * distance).sin()
                        / (std::f64::consts::PI * distance)
                };
                let angle = std::f64::consts::PI * distance / half as f64;
                *coefficient = sinc * (0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos());
            }
            let gain: f64 = kernel.iter().sum();
            for coefficient in kernel {
                *coefficient /= gain;
            }
        }
        Self {
            coefficients: coefficients.into_boxed_slice(),
            history: vec![0.0; length].into_boxed_slice(),
            cursor: 0,
            half,
            phase_resolution,
        }
    }

    fn push(&mut self, sample: i16) {
        self.history[self.cursor] = f64::from(sample);
        self.cursor += 1;
        if self.cursor == self.history.len() {
            self.cursor = 0;
        }
    }

    fn sample(&self, position: u64, output_rate: u64) -> i16 {
        let phase_position = position * self.phase_resolution;
        let phase = (phase_position / output_rate) as usize;
        let remainder = phase_position % output_rate;
        let length = self.history.len();
        let left = &self.coefficients[phase * length..(phase + 1) * length];
        let (older, newer) = self.history.split_at(self.cursor);
        // The cursor points at the oldest frame. The centered kernel evaluates
        // the requested fractional time directly, with no linear interpolation
        // of already filtered/rounded samples (which rolled off 44.1 kHz audio).
        let samples = newer.iter().chain(older);
        let value = if remainder == 0 {
            left.iter()
                .zip(samples)
                .map(|(coefficient, sample)| coefficient * sample)
                .sum::<f64>()
        } else {
            let right = &self.coefficients[(phase + 1) * length..(phase + 2) * length];
            let fraction = remainder as f64 / output_rate as f64;
            left.iter()
                .zip(right)
                .zip(samples)
                .map(|((left, right), sample)| (left + (right - left) * fraction) * sample)
                .sum::<f64>()
        };
        // Preserve precision and headroom until the one final PCM conversion.
        value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
    }
}

impl StreamingResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        assert!(input_rate > 0 && output_rate > 0);
        let low_pass = (input_rate > output_rate).then(|| LowPass::new(input_rate, output_rate));
        // Wait for the filter's lookahead instead of inserting artificial leading
        // silence into the sample clock. The first emitted frame represents t=0.
        let position = low_pass
            .as_ref()
            .map_or(0, |filter| filter.half as u64 * u64::from(output_rate));
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
        self.low_pass.as_ref().map_or(0, |filter| filter.half)
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
        let zeros = [0; 1026];
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
                filter.push(sample);
                // Rational arithmetic keeps time exact across arbitrary packet
                // splits. The FIR runs only for output frames, not every input.
                while self.position < self.output_rate {
                    output.push(filter.sample(self.position, self.output_rate));
                    self.position += self.input_rate;
                }
                self.position -= self.output_rate;
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
        (0..rate / 4)
            .map(|index| {
                (24_000.0
                    * (2.0 * std::f64::consts::PI * frequency * index as f64 / rate as f64
                        + std::f64::consts::FRAC_PI_4)
                        .sin())
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
        // Whole-cycle 100 ms windows exclude both finite-source boundaries.
        // A pi/4 phase also detects energy at the output Nyquist: a zero-phase
        // sine there would misleadingly vanish after decimation by itself.
        let output_start = output_rate as usize / 20;
        let input_start = input_rate as usize / 20;
        let amplitude = rms(&output[output_start..output_start + output_rate as usize / 10]);
        let reference = rms(&input[input_start..input_start + input_rate as usize / 10]);
        20.0 * (amplitude / reference).log10()
    }

    // Independent non-streaming definition: sum against the original finite
    // input at each exact requested time. This oracle has no ring, phase bank,
    // packet state, causal delay clock or intermediate PCM quantization.
    fn reference_downsample(input: &[i16], input_rate: u32, output_rate: u32) -> Vec<i16> {
        let count = (input.len().saturating_sub(1) as u64 * u64::from(output_rate))
            .div_ceil(u64::from(input_rate));
        let half = (48 * u64::from(input_rate) / u64::from(output_rate)).clamp(32, 1024) as i64;
        let cutoff = 15.0 * f64::from(output_rate) / (32.0 * f64::from(input_rate));
        (0..count)
            .map(|index| {
                let time = index * u64::from(input_rate);
                let center = (time / u64::from(output_rate)) as i64;
                let fraction = (time % u64::from(output_rate)) as f64 / f64::from(output_rate);
                let mut weighted = 0.0;
                let mut gain = 0.0;
                for offset in -half..=half {
                    let distance = offset as f64 - fraction;
                    if distance.abs() > half as f64 {
                        continue;
                    }
                    let argument = 2.0 * std::f64::consts::PI * cutoff * distance;
                    let sinc = if argument == 0.0 {
                        1.0
                    } else {
                        argument.sin() / argument
                    };
                    let window_position = (distance + half as f64) / (2 * half) as f64;
                    let angle = 2.0 * std::f64::consts::PI * window_position;
                    let coefficient = 2.0
                        * cutoff
                        * sinc
                        * (0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos());
                    gain += coefficient;
                    let source = center + offset;
                    if source >= 0 {
                        weighted += coefficient
                            * input.get(source as usize).copied().map_or(0.0, f64::from);
                    }
                }
                (weighted / gain)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16
            })
            .collect()
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
            let mut tail = Vec::new();
            whole.finish(&mut tail);
            expected.extend_from_slice(&tail);
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
                split.finish(&mut chunk_output);
                actual.extend_from_slice(&chunk_output);
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
    fn rejects_the_whole_stopband_at_common_capture_rates() {
        for input_rate in [44_100, 48_000, 96_000, 192_000] {
            for frequency in [8_000.0, 8_200.0, 8_500.0, 9_000.0, 12_000.0, 15_000.0] {
                let gain = gain_db(input_rate, 16_000, frequency);
                assert!(
                    gain <= -60.0,
                    "{input_rate}/16000 at {frequency} Hz: {gain:.2} dB"
                );
            }
        }
    }

    #[test]
    fn preserves_speech_passband() {
        for input_rate in [44_100, 48_000, 96_000, 192_000] {
            for frequency in [100.0, 1_000.0, 3_000.0, 5_000.0, 6_500.0, 7_000.0] {
                let gain = gain_db(input_rate, 16_000, frequency);
                assert!(
                    gain.abs() <= 0.1,
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
            assert_eq!(first_frame, preview.delay_input_samples());
            let mut resampler = StreamingResampler::new(input_rate, 16_000);
            let delay_ms = 1000.0 * resampler.delay_input_samples() as f64 / input_rate as f64;
            assert!(delay_ms <= 3.1, "{input_rate}: {delay_ms} ms");
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

    #[test]
    fn finite_sources_match_the_direct_fractional_convolution_oracle() {
        for input_rate in [44_100, 44_101, 48_000, 96_000, 192_000] {
            let delay = StreamingResampler::new(input_rate, 16_000).delay_input_samples();
            for length in [0, 1, 2, delay - 1, delay, delay + 1, 3 * delay + 17] {
                let mut input: Vec<_> = (0..length)
                    .map(|index| (((index * 12_413 + 2_789) % 48_001) as i32 - 24_000) as i16)
                    .collect();
                if let Some(first) = input.first_mut() {
                    *first = i16::MIN;
                }
                if let Some(last) = input.last_mut() {
                    *last = i16::MAX;
                }
                let expected = reference_downsample(&input, input_rate, 16_000);
                for packet_size in [1, 137, length.max(1)] {
                    let mut actual = Vec::new();
                    let mut chunk_output = Vec::new();
                    let mut resampler = StreamingResampler::new(input_rate, 16_000);
                    for packet in input.chunks(packet_size) {
                        resampler.process(packet, &mut chunk_output);
                        actual.extend_from_slice(&chunk_output);
                    }
                    resampler.finish(&mut chunk_output);
                    actual.extend_from_slice(&chunk_output);
                    assert_eq!(actual.len(), expected.len(), "{input_rate}, {length}");
                    for (index, (&actual, &expected)) in actual.iter().zip(&expected).enumerate() {
                        assert!(
                            (i32::from(actual) - i32::from(expected)).abs() <= 1,
                            "{input_rate}, {length}, packet {packet_size}, frame {index}: \
                             {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fractional_conversion_preserves_the_requested_sine_phase() {
        for input_rate in [44_100, 44_101] {
            for frequency in [1_000.0, 5_000.0, 7_000.0] {
                let input = tone(input_rate, frequency);
                let mut output = Vec::new();
                StreamingResampler::new(input_rate, 16_000).process(&input, &mut output);
                let mut error = 0.0;
                for (index, &actual) in output.iter().enumerate().skip(800).take(1_600) {
                    let expected = 24_000.0
                        * (2.0 * std::f64::consts::PI * frequency * index as f64 / 16_000.0
                            + std::f64::consts::FRAC_PI_4)
                            .sin();
                    error += (f64::from(actual) - expected).powi(2);
                }
                let error_rms = (error / 1_600.0).sqrt();
                assert!(error_rms < 5.0, "{input_rate}, {frequency}: {error_rms}");
            }
        }
    }

    #[test]
    fn atypical_rates_keep_constructor_storage_bounded() {
        for (input_rate, output_rate) in [(44_101, 16_000), (u32::MAX, 1), (u32::MAX, 16_001)] {
            let filter = LowPass::new(input_rate, output_rate);
            assert!(filter.half <= 1024);
            assert!(filter.coefficients.len() <= 257 * 2049);
            assert!(filter.coefficients.iter().all(|sample| sample.is_finite()));
        }
    }
}
