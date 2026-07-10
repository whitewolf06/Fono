//! Shared application audio entrypoint.
//!
//! Capture, format selection, channel mixing and resampling live in
//! `fono-wake::audio_source`, so dictation, microphone tests and wake word all
//! receive the same mono 16 kHz PCM stream.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::error::{AppError, AppResult};
use crate::types::DeviceInfo;

pub type AudioInputStream = fono_wake::audio_source::AudioStream;

pub struct AudioCapture;

impl AudioCapture {
    pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().and_then(|device| device.name().ok());

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

    /// Start the same normalized mono 16 kHz input used by wake word.
    pub fn start<F>(device_id: Option<&str>, on_samples: F) -> AppResult<AudioInputStream>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        tracing::info!("AudioCapture::start: shared input, device_id={device_id:?}");
        AudioInputStream::start(device_id, 16_000, on_samples).map_err(|error| {
            tracing::error!("AudioCapture::start failed: {error}");
            AppError::Audio(error.to_string())
        })
    }
}
