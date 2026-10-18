pub fn read(path: &str) -> Result<Vec<i16>, String> {
    read_at_rate(path, 16_000)
}

pub fn read_at_rate(path: &str, expected_rate: u32) -> Result<Vec<i16>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    decode(&bytes, expected_rate)
}

fn decode(bytes: &[u8], expected_rate: u32) -> Result<Vec<i16>, String> {
    let invalid_format = || format!("expected PCM16 mono {expected_rate} Hz WAV");
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(invalid_format());
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
                && u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap())
                    == expected_rate
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
        return Err(invalid_format());
    }
    samples.ok_or("missing data".into())
}

#[cfg(test)]
mod tests {
    use super::decode;

    fn wav_at_rate(rate: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(b"\x02\0\x10\0data");
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&123_i16.to_le_bytes());
        bytes.extend_from_slice(&(-456_i16).to_le_bytes());
        bytes
    }

    #[test]
    fn native_rate_must_match_the_declared_replay_clock() {
        let native = wav_at_rate(48_000);
        assert_eq!(decode(&native, 48_000).unwrap(), [123, -456]);
        assert!(decode(&native, 16_000).is_err());
        assert!(decode(&wav_at_rate(16_000), 48_000).is_err());
    }

    #[test]
    fn native_input_still_rejects_wrong_format_and_truncated_samples() {
        let mut stereo = wav_at_rate(48_000);
        stereo[22] = 2;
        assert!(decode(&stereo, 48_000).is_err());
        let mut native = wav_at_rate(48_000);
        native.pop();
        assert!(decode(&native, 48_000).is_err());
    }
}

pub fn write(path: &str, samples: &[i16]) -> Result<(), String> {
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
