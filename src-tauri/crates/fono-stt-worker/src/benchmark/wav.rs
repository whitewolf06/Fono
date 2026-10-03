use std::fs;
use std::path::Path;

/// Explicit input contract avoids silently resampling a benchmark fixture.
pub fn load(path: &Path) -> Result<Vec<i16>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected RIFF/WAVE".into());
    }
    let mut offset = 12;
    let mut valid_format = false;
    let mut audio = None;
    while offset + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start.checked_add(length).ok_or("WAV chunk overflow")?;
        if end > bytes.len() {
            return Err("truncated WAV chunk".into());
        }
        match &bytes[offset..offset + 4] {
            b"fmt " if length >= 16 => {
                let pcm = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
                let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
                let rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
                let bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
                valid_format = pcm == 1 && channels == 1 && rate == 16000 && bits == 16;
            }
            b"data" => audio = Some(&bytes[start..end]),
            _ => {}
        }
        offset = end + length % 2;
    }
    if !valid_format {
        return Err("benchmark requires PCM16, mono, 16000 Hz WAV".into());
    }
    let audio = audio.ok_or("WAV has no data chunk")?;
    if audio.is_empty() || audio.len() % 2 != 0 {
        return Err("empty or malformed PCM16 data".into());
    }
    Ok(audio
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}
