//! One physical input, ordered sample clocks and isolated bounded consumers.
mod capture;
mod convert;
mod hub;
mod owner;
pub mod resample;
mod ring;

pub use capture::AudioStream;
pub use hub::{AudioHub, AudioHubDiagnostics, AudioSubscription};
pub use ring::{AudioCursor, AudioPacket};

// A small forward distance across u64::MAX is a wrap; a backwards jump is a
// counter reset and cannot be interpreted as a number of lost packets.
fn capture_loss_delta(baseline: u64, current: u64) -> Option<u64> {
    if current >= baseline {
        Some(current - baseline)
    } else if baseline - current > u64::MAX / 2 {
        Some(current.wrapping_sub(baseline))
    } else {
        None
    }
}
