//! Final speech evidence complements the neural detector's streaming latch.
//! A click may leave the detector in its silence hangover; its energy is still
//! concentrated in a few samples, unlike even a quiet, short spoken word.

const FRAME_SAMPLES: usize = 320; // 20 ms at the shared 16 kHz sample rate.
const MIN_ACTIVE_FRAMES: usize = 4;
const MIN_CONSECUTIVE_FRAMES: usize = 3;

#[derive(Default)]
struct FrameEnergy {
    energy: f64,
    peak_squared: f64,
}

fn energy(samples: &[f32]) -> FrameEnergy {
    if samples.is_empty() || samples.iter().any(|sample| !sample.is_finite()) {
        return FrameEnergy::default();
    }
    let mean = samples.iter().map(|&sample| sample as f64).sum::<f64>() / samples.len() as f64;
    let mut result = FrameEnergy::default();
    for &sample in samples {
        let squared = (sample as f64 - mean).powi(2);
        result.energy += squared;
        result.peak_squared = result.peak_squared.max(squared);
    }
    result
}

/// Relative energy only: no fixed RMS floor that would reject a quiet voice.
/// The second strongest frame prevents one loud key click from setting the
/// reference for a quieter word. Neural VAD remains mandatory; this test cannot
/// turn sustained music, fan noise or a tone into speech on its own.
pub(super) fn sustained_signal(samples: &[f32]) -> bool {
    let mut largest = 0.0_f64;
    let mut second = 0.0_f64;
    for frame in samples.chunks(FRAME_SAMPLES) {
        let value = energy(frame).energy;
        if value > largest {
            second = largest;
            largest = value;
        } else {
            second = second.max(value);
        }
    }
    if second <= 0.0 {
        return false;
    }
    let mut active = 0;
    let mut consecutive = 0;
    for frame in samples.chunks(FRAME_SAMPLES) {
        let value = energy(frame);
        // A nearly empty final frame and sparse impulses are not 20 ms of voice.
        let occupied = frame.len() >= FRAME_SAMPLES / 2
            && value.energy >= second * 0.01
            && value.energy > value.peak_squared * 4.0;
        consecutive = if occupied { consecutive + 1 } else { 0 };
        active += usize::from(occupied);
        if active >= MIN_ACTIVE_FRAMES && consecutive >= MIN_CONSECUTIVE_FRAMES {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voiced(frames: usize, scale: f32) -> Vec<f32> {
        (0..frames * FRAME_SAMPLES)
            .map(|index| (index as f32 * 0.085).sin() * scale)
            .collect()
    }

    #[test]
    fn silence_and_dc_offset_have_no_sustained_signal() {
        assert!(!sustained_signal(&vec![0.0; 16_000]));
        assert!(!sustained_signal(&vec![0.2; 16_000]));
    }

    #[test]
    fn one_or_sporadic_clicks_are_not_voice() {
        let mut samples = vec![0.0; 16_000];
        for index in [0, 5_000, 9_000, 13_000] {
            samples[index] = 1.0;
        }
        assert!(!sustained_signal(&samples));
        samples.fill(0.0);
        samples[600..700].copy_from_slice(&voiced(1, 1.0)[..100]);
        assert!(!sustained_signal(&samples));
    }

    #[test]
    fn quiet_short_voice_and_a_loud_leading_click_are_retained() {
        let mut samples = voiced(5, 0.0001);
        assert!(sustained_signal(&samples));
        samples[0] = 1.0;
        assert!(sustained_signal(&samples));
    }

    #[test]
    fn a_single_active_frame_or_invalid_audio_is_not_voice() {
        assert!(!sustained_signal(&voiced(1, 0.2)));
        assert!(!sustained_signal(&vec![f32::NAN; 16_000]));
    }
}
