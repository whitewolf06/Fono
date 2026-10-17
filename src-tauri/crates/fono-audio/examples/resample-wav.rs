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

#[path = "support/pcm_wav.rs"]
mod pcm_wav;
use pcm_wav::{read, write};

fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() != 5 || !["legacy", "current"].contains(&arguments[3].as_str()) {
        return Err(
            "usage: resample-wav INPUT16k.wav OUTPUT16k.wav legacy|current REPLAY_RATE".into(),
        );
    }
    let rate: u32 = arguments[4].parse().map_err(|_| "invalid replay rate")?;
    if ![44_100, 48_000, 96_000].contains(&rate) {
        return Err("unsupported replay rate".into());
    }
    // Reconstruction is intentionally identical for both alternatives, and is
    // recorded as derived replay; it cannot create native wideband evidence.
    let mut input = legacy(&read(&arguments[1])?, 16000, rate);
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
    let seconds = started.elapsed().as_secs_f64();
    write(&arguments[2], &output)?;
    println!(
        "{{\"resample_seconds\":{seconds},\"input_rate\":{rate},\"output_samples\":{}}}",
        output.len()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
