/// Linear streaming resampler. Fractional position and the boundary sample
/// survive every callback; arbitrary packet sizes cannot change the waveform.
pub struct StreamingResampler {
    input_rate: u64,
    output_rate: u64,
    position: u64,
    previous: Option<i16>,
}

impl StreamingResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        assert!(input_rate > 0 && output_rate > 0);
        Self {
            input_rate: input_rate as u64,
            output_rate: output_rate as u64,
            position: 0,
            previous: None,
        }
    }

    pub fn process(&mut self, input: &[i16], output: &mut Vec<i16>) {
        output.clear();
        if input.is_empty() {
            return;
        }
        if self.input_rate == self.output_rate {
            output.extend_from_slice(input);
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
    #[test]
    fn packet_boundaries_do_not_change_output() {
        for (input_rate, output_rate) in [
            (44_100, 16_000),
            (48_000, 16_000),
            (16_000, 8_000),
            (8_000, 16_000),
        ] {
            let input: Vec<i16> = (0..10001)
                .map(|i| ((i * 127) % 20000 - 10000) as i16)
                .collect();
            let mut whole = StreamingResampler::new(input_rate, output_rate);
            let mut expected = Vec::new();
            whole.process(&input, &mut expected);
            let mut split = StreamingResampler::new(input_rate, output_rate);
            let mut actual = Vec::new();
            let mut chunk_output = Vec::new();
            for chunk in input.chunks(137) {
                split.process(chunk, &mut chunk_output);
                actual.extend_from_slice(&chunk_output);
            }
            assert_eq!(expected, actual, "{input_rate}/{output_rate}");
        }
    }
}
