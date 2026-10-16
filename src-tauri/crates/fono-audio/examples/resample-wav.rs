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

fn read(path: &str) -> Result<Vec<i16>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected PCM16 mono 16 kHz WAV".into());
    }
    let mut position = 12;
    let mut valid = false;
    let mut samples = None;
    while position + 8 <= bytes.len() {
        let size =
            u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
        let start = position + 8;
        let end = start.checked_add(size).ok_or("chunk overflow")?;
        if end > bytes.len() {
            return Err("truncated WAV".into());
        }
        if &bytes[position..position + 4] == b"fmt " && size >= 16 {
            valid = bytes[start..start + 4] == [1, 0, 1, 0]
                && u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()) == 16000
                && bytes[start + 14..start + 16] == [16, 0];
        }
        if &bytes[position..position + 4] == b"data" {
            if size % 2 != 0 {
                return Err("odd PCM16 data".into());
            }
            samples = Some(
                bytes[start..end]
                    .chunks_exact(2)
                    .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                    .collect(),
            );
        }
        position = end + size % 2;
    }
    if !valid {
        return Err("expected PCM16 mono 16 kHz WAV".into());
    }
    samples.ok_or("missing data".into())
}

fn write(path: &str, samples: &[i16]) -> Result<(), String> {
    let size = u32::try_from(samples.len() * 2).map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(size as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&32000_u32.to_le_bytes());
    bytes.extend_from_slice(b"\x02\0\x10\0data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

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
