//! Offline replay fixture converter. The frozen baseline is the pre-PERF-04
//! interpolation, not a second independently changing implementation.
use fono_audio::audio_source::resample::StreamingResampler;

fn legacy(input: &[i16], input_rate: u32, output_rate: u32) -> Vec<i16> {
    if input_rate == output_rate {
        return input.to_vec();
    }
    let mut output = Vec::new();
    let mut position = 0_u64;
    let boundary = input.len().saturating_sub(1) as u64 * u64::from(output_rate);
    while position < boundary {
        let index = (position / u64::from(output_rate)) as usize;
        let fraction = (position % u64::from(output_rate)) as f64 / f64::from(output_rate);
        let a = f64::from(input[index]);
        let b = f64::from(input[index + 1]);
        output.push((a + (b - a) * fraction).round() as i16);
        position += u64::from(input_rate);
    }
    output
}

/// Additional derived replay oracle, fixed independently of speech outcomes.
///
/// A cardinal sinc at the original 16 kHz Nyquist with a Blackman support of
/// +/-48 source frames suppresses the linear reconstruction's image spectrum.
/// Integer source positions remain exact, so 16->48->legacy16 keeps the frozen
/// legacy PCM. This is a distinct fixture mode, not native wideband evidence.
fn bandlimited_reconstruction(input: &[i16], output_rate: u32) -> Vec<i16> {
    const HALF: i64 = 48;
    const SOURCE_RATE: u64 = 16_000;
    let gcd = |mut a: u32, mut b: u32| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let phase_count = output_rate / gcd(output_rate, SOURCE_RATE as u32);
    let phase_step = output_rate / phase_count;
    let phases: Vec<Vec<f64>> = (0..phase_count)
        .map(|phase| {
            let fraction = f64::from(phase) / f64::from(phase_count);
            let mut coefficients: Vec<f64> = (-HALF..=HALF)
                .map(|offset| {
                    let distance = offset as f64 - fraction;
                    if distance.abs() >= HALF as f64 {
                        return 0.0;
                    }
                    let sinc = if distance == 0.0 {
                        1.0
                    } else {
                        let angle = std::f64::consts::PI * distance;
                        angle.sin() / angle
                    };
                    let angle = std::f64::consts::PI * distance / HALF as f64;
                    sinc * (0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos())
                })
                .collect();
            let gain: f64 = coefficients.iter().sum();
            for coefficient in &mut coefficients {
                *coefficient /= gain;
            }
            coefficients
        })
        .collect();
    let boundary = input.len().saturating_sub(1) as u64 * u64::from(output_rate);
    let mut position = 0_u64;
    let mut output = Vec::with_capacity(boundary.div_ceil(SOURCE_RATE) as usize);
    while position < boundary {
        let index = (position / u64::from(output_rate)) as i64;
        let remainder = position % u64::from(output_rate);
        if remainder == 0 {
            // Preserve the cardinal property exactly, including PCM rounding.
            output.push(input[index as usize]);
        } else {
            let phase = (remainder / u64::from(phase_step)) as usize;
            let mut value = 0.0;
            for (offset, coefficient) in (-HALF..=HALF).zip(&phases[phase]) {
                let source = index + offset;
                if source >= 0 && (source as usize) < input.len() {
                    value += coefficient * f64::from(input[source as usize]);
                }
            }
            output.push(value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
        }
        position += SOURCE_RATE;
    }
    output
}

#[path = "support/pcm_wav.rs"]
mod pcm_wav;
use pcm_wav::{read, read_at_rate, write};

fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().collect();
    if !(5..=6).contains(&arguments.len())
        || !["legacy", "current"].contains(&arguments[3].as_str())
        || (arguments.len() == 6
            && !["--native-input", "--bandlimited-reconstruction"].contains(&arguments[5].as_str()))
    {
        return Err(
            "usage: resample-wav INPUT.wav OUTPUT16k.wav legacy|current REPLAY_RATE [--native-input|--bandlimited-reconstruction]"
                .into(),
        );
    }
    let rate: u32 = arguments[4].parse().map_err(|_| "invalid replay rate")?;
    if ![44_100, 48_000, 96_000].contains(&rate) {
        return Err("unsupported replay rate".into());
    }
    let native_input = arguments
        .get(5)
        .is_some_and(|flag| flag == "--native-input");
    let bandlimited = arguments
        .get(5)
        .is_some_and(|flag| flag == "--bandlimited-reconstruction");
    let mut input = if native_input {
        // A native fixture must declare the exact input clock. It is never
        // silently reconstructed from a 16 kHz source or rate-converted here.
        read_at_rate(&arguments[1], rate)?
    } else if bandlimited {
        bandlimited_reconstruction(&read(&arguments[1])?, rate)
    } else {
        // Keep the historical derived replay byte-for-byte: reconstruction is
        // identical for both alternatives and does not prove native bandwidth.
        legacy(&read(&arguments[1])?, 16000, rate)
    };
    let expected =
        ((input.len().saturating_sub(1) as u64 * 16000).div_ceil(u64::from(rate))) as usize;
    // Equal finite-source zero extension for both alternatives, followed by the
    // exact original duration. This also works with the pre-FIR constructor, so
    // the quality infrastructure can precede the production filter change.
    input.resize(input.len() + rate as usize / 100, 0);
    let started = std::time::Instant::now();
    let mut output = if arguments[3] == "legacy" {
        legacy(&input, rate, 16000)
    } else {
        let mut resampler = StreamingResampler::new(rate, 16000);
        let mut output = Vec::with_capacity(input.len() * 16000 / rate as usize + 2);
        let mut complete = Vec::new();
        for packet in input.chunks(137) {
            resampler.process(packet, &mut output);
            complete.extend_from_slice(&output);
        }
        complete
    };
    output.truncate(expected);
    if output.len() != expected {
        return Err("resampler did not complete the expected finite source".into());
    }
    let seconds = started.elapsed().as_secs_f64();
    write(&arguments[2], &output)?;
    let source_mode = if native_input {
        "native"
    } else if bandlimited {
        "bandlimited-reconstruction"
    } else {
        "historical-linear-reconstruction"
    };
    println!(
        "{{\"resample_seconds\":{seconds},\"input_rate\":{rate},\"output_samples\":{},\"native_input\":{native_input},\"source_mode\":\"{source_mode}\"}}",
        output.len(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bandlimited_reconstruction, legacy};

    #[test]
    fn cardinal_oracle_keeps_frozen_48khz_legacy_pcm_and_duration() {
        let input: Vec<i16> = (0..10_001)
            .map(|index| ((index * 127) % 20_000 - 10_000) as i16)
            .collect();
        let historical = legacy(&input, 16_000, 48_000);
        let oracle = bandlimited_reconstruction(&input, 48_000);
        assert_eq!(historical.len(), oracle.len());
        assert_eq!(
            legacy(&historical, 48_000, 16_000),
            legacy(&oracle, 48_000, 16_000)
        );
        assert_ne!(
            historical, oracle,
            "intermediate image spectrum must change"
        );
    }

    #[test]
    fn bandlimited_oracle_has_the_historical_finite_clock_at_each_rate() {
        for rate in [44_100, 48_000, 96_000] {
            for length in [0, 1, 2, 17, 150] {
                let input = vec![1_000; length];
                let oracle = bandlimited_reconstruction(&input, rate);
                assert_eq!(oracle.len(), legacy(&input, 16_000, rate).len());
                if length == 150 {
                    let start = 60 * rate as usize / 16_000;
                    let end = 90 * rate as usize / 16_000;
                    assert!(oracle[start..end].iter().all(|&sample| sample == 1_000));
                }
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
