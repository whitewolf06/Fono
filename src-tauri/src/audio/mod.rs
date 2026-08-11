//! Shared application audio entrypoint.
//!
//! Capture, format selection, channel mixing and resampling live in
//! `fono-wake::audio_source`. `AudioHub` owns one physical stream and fans
//! its mono 16 kHz PCM frames out to wake word and dictation subscribers.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::DeviceInfo;

pub type RecordingWriter = Arc<Mutex<Vec<i16>>>;
pub(crate) use fono_core::AudioCapturePort as AudioRecorder;

pub struct AudioCapture;

impl AudioCapture {
    pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device.name().ok());

        let mut devices = Vec::new();
        if let Ok(inputs) = host.input_devices() {
            for device in inputs {
                if let Ok(name) = device.name() {
                    devices.push(DeviceInfo {
                        id: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                        name,
                    });
                }
            }
        }
        Ok(devices)
    }
}

/// Dictation's subscription to the process-wide physical input stream.
pub struct AudioRecordingOwner {
    audio_hub: fono_wake::AudioHub,
    subscription: Mutex<Option<fono_wake::AudioSubscription>>,
}

impl AudioRecordingOwner {
    pub fn new(audio_hub: fono_wake::AudioHub) -> Self {
        Self {
            audio_hub,
            subscription: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        device_id: Option<&str>,
        writer: RecordingWriter,
        limit_reached: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
        maximum_samples: usize,
    ) -> AppResult<()> {
        let subscription = self
            .audio_hub
            .subscribe(device_id, 16_000, move |chunk: &[i16]| {
                let sum_sq: f64 = chunk
                    .iter()
                    .map(|&sample| {
                        let normalized = sample as f64 / i16::MAX as f64;
                        normalized * normalized
                    })
                    .sum();
                let rms = if chunk.is_empty() {
                    0.0
                } else {
                    (sum_sq / chunk.len() as f64).sqrt() as f32
                };
                level_bits.store(rms.to_bits(), Ordering::Relaxed);
                let mut samples = writer.lock();
                let available = maximum_samples.saturating_sub(samples.len());
                let accepted = available.min(chunk.len());
                samples.extend_from_slice(&chunk[..accepted]);
                if accepted < chunk.len() {
                    limit_reached.store(true, Ordering::Relaxed);
                }
            })
            .map_err(|error| AppError::Audio(error.to_string()))?;
        *self.subscription.lock() = Some(subscription);
        Ok(())
    }

    pub fn stop(&self) -> AppResult<()> {
        self.subscription.lock().take();
        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.stop();
    }
}

impl Drop for AudioRecordingOwner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl AudioRecorder<RecordingWriter, AppError> for AudioRecordingOwner {
    fn start(
        &self,
        device_id: Option<&str>,
        writer: RecordingWriter,
        limit_reached: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
        maximum_samples: usize,
    ) -> AppResult<()> {
        Self::start(
            self,
            device_id,
            writer,
            limit_reached,
            level_bits,
            maximum_samples,
        )
    }

    fn stop(&self) -> AppResult<()> {
        Self::stop(self)
    }

    fn shutdown(&self) {
        Self::shutdown(self);
    }
}

#[cfg(test)]
mod tests {
    use super::AudioRecordingOwner;

    #[test]
    fn owner_thread_shuts_down_without_an_active_stream() {
        let owner = AudioRecordingOwner::new(fono_wake::AudioHub::new());
        drop(owner);
    }
}
