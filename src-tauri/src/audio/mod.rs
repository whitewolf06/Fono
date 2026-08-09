//! Shared application audio entrypoint.
//!
//! Capture, format selection, channel mixing and resampling live in
//! `fono-wake::audio_source`, so dictation, microphone tests and wake word all
//! receive the same mono 16 kHz PCM stream.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::types::DeviceInfo;

pub type AudioInputStream = fono_wake::audio_source::AudioStream;
pub type RecordingWriter = Arc<Mutex<Vec<i16>>>;

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

/// Owns the non-Send CPAL stream on one dedicated thread.
///
/// Pipeline code only receives a command sender, which is safe to share between
/// Tauri command handlers. `Stop` drops the CPAL stream on this owner thread
/// before the recording buffer is returned to a caller.
pub struct AudioRecordingOwner {
    commands: Sender<AudioOwnerCommand>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

enum AudioOwnerCommand {
    Start {
        device_id: Option<String>,
        writer: RecordingWriter,
        limit_reached: Arc<AtomicBool>,
        maximum_samples: usize,
        response: Sender<AppResult<()>>,
    },
    Stop {
        response: Sender<()>,
    },
    Shutdown {
        response: Sender<()>,
    },
}

impl AudioRecordingOwner {
    pub fn new() -> Self {
        let (commands, receiver) = bounded(4);
        let thread = thread::spawn(move || run_audio_owner(receiver));
        Self {
            commands,
            thread: Mutex::new(Some(thread)),
        }
    }

    pub fn start(
        &self,
        device_id: Option<&str>,
        writer: RecordingWriter,
        limit_reached: Arc<AtomicBool>,
        maximum_samples: usize,
    ) -> AppResult<()> {
        let (response_tx, response_rx) = bounded(1);
        self.commands
            .send(AudioOwnerCommand::Start {
                device_id: device_id.map(str::to_owned),
                writer,
                limit_reached,
                maximum_samples,
                response: response_tx,
            })
            .map_err(|_| AppError::Audio("audio owner thread is unavailable".into()))?;
        response_rx
            .recv()
            .map_err(|_| AppError::Audio("audio owner stopped before start completed".into()))?
    }

    pub fn stop(&self) -> AppResult<()> {
        let (response_tx, response_rx) = bounded(1);
        self.commands
            .send(AudioOwnerCommand::Stop {
                response: response_tx,
            })
            .map_err(|_| AppError::Audio("audio owner thread is unavailable".into()))?;
        response_rx
            .recv()
            .map_err(|_| AppError::Audio("audio owner stopped before stop completed".into()))
    }
}

impl Default for AudioRecordingOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioRecordingOwner {
    fn drop(&mut self) {
        let (response_tx, response_rx) = bounded(1);
        let _ = self.commands.send(AudioOwnerCommand::Shutdown {
            response: response_tx,
        });
        let _ = response_rx.recv();
        if let Some(thread) = self.thread.get_mut().take() {
            let _ = thread.join();
        }
    }
}

fn run_audio_owner(receiver: Receiver<AudioOwnerCommand>) {
    let mut stream: Option<AudioInputStream> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            AudioOwnerCommand::Start {
                device_id,
                writer,
                limit_reached,
                maximum_samples,
                response,
            } => {
                drop(stream.take());
                limit_reached.store(false, Ordering::SeqCst);
                let result = AudioCapture::start(device_id.as_deref(), move |chunk: &[i16]| {
                    let mut samples = writer.lock();
                    let available = maximum_samples.saturating_sub(samples.len());
                    let accepted = available.min(chunk.len());
                    samples.extend_from_slice(&chunk[..accepted]);
                    if accepted < chunk.len() {
                        limit_reached.store(true, Ordering::Relaxed);
                    }
                });
                match result {
                    Ok(started) => {
                        stream = Some(started);
                        let _ = response.send(Ok(()));
                    }
                    Err(error) => {
                        let _ = response.send(Err(error));
                    }
                }
            }
            AudioOwnerCommand::Stop { response } => {
                drop(stream.take());
                let _ = response.send(());
            }
            AudioOwnerCommand::Shutdown { response } => {
                drop(stream.take());
                let _ = response.send(());
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AudioRecordingOwner;

    #[test]
    fn owner_thread_shuts_down_without_an_active_stream() {
        let owner = AudioRecordingOwner::new();
        drop(owner);
    }
}
