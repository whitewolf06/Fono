pub fn read(path: &str) -> Result<Vec<i16>, String> {
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
