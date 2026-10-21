//! Small generated PCM cues; playback uses a separate synchronous output path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cue {
    Start,
    Stop,
    Error,
}

const SAMPLE_RATE: u32 = 22_050;
const SILENT_TAIL_MS: usize = 50;

fn wave(cues: &[Cue], volume: u8) -> Vec<u8> {
    let mut pcm = Vec::<i16>::new();
    let amplitude = 0.18 * f64::from(volume.min(100)) / 100.0;
    for cue in cues {
        let duration_ms = if *cue == Cue::Error { 90 } else { 60 };
        let count = SAMPLE_RATE as usize * duration_ms / 1_000;
        let seconds = count as f64 / f64::from(SAMPLE_RATE);
        let (from, to) = match cue {
            Cue::Start => (500.0, 760.0),
            Cue::Stop => (760.0, 500.0),
            Cue::Error => (220.0, 170.0),
        };
        for i in 0..count {
            let t = i as f64 / f64::from(SAMPLE_RATE);
            let phase = std::f64::consts::TAU * (from * t + (to - from) * t * t / (2.0 * seconds));
            let fade = ((i.min(count - 1 - i)) as f64 / (f64::from(SAMPLE_RATE) * 0.005)).min(1.0);
            pcm.push((phase.sin() * fade * amplitude * f64::from(i16::MAX)) as i16);
        }
        pcm.resize(pcm.len() + SAMPLE_RATE as usize * SILENT_TAIL_MS / 1_000, 0);
    }
    let data_len = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Called only from an admitted blocking worker, never from an audio callback.
pub(crate) fn play(cues: &[Cue], volume: u8) -> bool {
    if volume == 0 || volume > 100 {
        return true;
    }
    let wav = wave(cues, volume);
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Media::Audio::{
            PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_NOSTOP, SND_SYNC,
        };
        // SND_MEMORY interprets this address as a WAV buffer, not a string.
        // SND_SYNC keeps it alive through the final sample and silent tail;
        // NODEFAULT prevents an unrelated Windows beep on failure.
        unsafe {
            PlaySoundW(
                PCWSTR(wav.as_ptr().cast()),
                None,
                SND_MEMORY | SND_SYNC | SND_NODEFAULT | SND_NOSTOP,
            )
            .as_bool()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = wav;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn samples(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect()
    }
    #[test]
    fn cues_are_distinct_quiet_bounded_pcm_with_a_silent_tail() {
        let start = wave(&[Cue::Start], 25);
        let stop = wave(&[Cue::Stop], 25);
        assert_ne!(start, stop);
        for cue in [Cue::Start, Cue::Stop, Cue::Error] {
            let wav = wave(&[cue], 100);
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            assert_eq!(
                u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize,
                wav.len() - 44
            );
            let pcm = samples(&wav);
            assert!((2_000..=3_200).contains(&pcm.len()));
            assert!(pcm.iter().any(|v| *v != 0));
            assert!(pcm.iter().all(|v| v.unsigned_abs() <= 5_900));
            assert!(pcm[pcm.len() - 1_102..].iter().all(|v| *v == 0));
        }
    }
    #[test]
    fn volume_changes_only_amplitude_and_zero_is_silent() {
        let quiet = samples(&wave(&[Cue::Start], 25));
        let loud = samples(&wave(&[Cue::Start], 100));
        assert_eq!(quiet.len(), loud.len());
        assert!(quiet
            .iter()
            .zip(loud)
            .all(|(a, b)| (i32::from(*a) * 4 - i32::from(b)).abs() <= 3));
        assert!(samples(&wave(&[Cue::Start, Cue::Stop, Cue::Error], 0))
            .iter()
            .all(|v| *v == 0));
    }
}
