//! Minimal CPAL-based audio capture used by wake word backends.
//!
//! Captures mono i16 samples at the requested sample rate (default 16 kHz).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::error::{WakeWordError, WakeWordResult};

pub struct AudioStream {
    _stream: cpal::Stream,
}

type AudioSubscriber = Arc<dyn Fn(&[i16]) + Send + Sync>;
type Subscribers = Arc<Mutex<BTreeMap<u64, AudioSubscriber>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AudioConfig {
    device_id: Option<String>,
    sample_rate: u32,
}

/// Owns one physical CPAL input stream and fans its normalised PCM frames out
/// to a bounded set of consumers. Wake word and dictation therefore share the
/// same device instead of reopening it for every transition.
#[derive(Clone)]
pub struct AudioHub {
    inner: Arc<AudioHubInner>,
}

struct AudioHubInner {
    commands: Sender<AudioHubCommand>,
    subscribers: Subscribers,
    next_subscription_id: AtomicU64,
    thread: Mutex<Option<JoinHandle<()>>>,
}

enum AudioHubCommand {
    Ensure {
        config: AudioConfig,
        response: Sender<WakeWordResult<()>>,
    },
    StopIfUnused,
    Shutdown {
        response: Sender<()>,
    },
}

/// RAII subscription to [`AudioHub`]. Dropping it detaches the consumer; the
/// hub stops the physical stream after the last subscriber disappears.
pub struct AudioSubscription {
    id: u64,
    inner: Arc<AudioHubInner>,
}

impl AudioHub {
    pub const MAX_SUBSCRIBERS: usize = 4;

    pub fn new() -> Self {
        let (commands, receiver) = bounded(8);
        let subscribers = Arc::new(Mutex::new(BTreeMap::new()));
        let owner_subscribers = Arc::clone(&subscribers);
        let thread = thread::spawn(move || run_audio_hub_owner(receiver, owner_subscribers));
        Self {
            inner: Arc::new(AudioHubInner {
                commands,
                subscribers,
                next_subscription_id: AtomicU64::new(1),
                thread: Mutex::new(Some(thread)),
            }),
        }
    }

    /// Attach a non-blocking consumer. All consumers on the hub must request
    /// the same input device and normalised sample rate.
    pub fn subscribe<F>(
        &self,
        device_id: Option<&str>,
        sample_rate: u32,
        callback: F,
    ) -> WakeWordResult<AudioSubscription>
    where
        F: Fn(&[i16]) + Send + Sync + 'static,
    {
        let id = self
            .inner
            .next_subscription_id
            .fetch_add(1, Ordering::Relaxed);
        {
            let mut subscribers = self.inner.subscribers.lock();
            if subscribers.len() >= Self::MAX_SUBSCRIBERS {
                return Err(WakeWordError::Audio(format!(
                    "audio hub subscriber limit ({}) reached",
                    Self::MAX_SUBSCRIBERS
                )));
            }
            subscribers.insert(id, Arc::new(callback));
        }

        let (response_tx, response_rx) = bounded(1);
        let config = AudioConfig {
            device_id: device_id.map(str::to_owned),
            sample_rate,
        };
        let result = self
            .inner
            .commands
            .send(AudioHubCommand::Ensure {
                config,
                response: response_tx,
            })
            .map_err(|_| WakeWordError::Audio("audio hub owner thread is unavailable".into()))
            .and_then(|_| {
                response_rx.recv().map_err(|_| {
                    WakeWordError::Audio("audio hub stopped before subscription completed".into())
                })?
            });

        if let Err(error) = result {
            self.inner.subscribers.lock().remove(&id);
            let _ = self.inner.commands.send(AudioHubCommand::StopIfUnused);
            return Err(error);
        }

        Ok(AudioSubscription {
            id,
            inner: Arc::clone(&self.inner),
        })
    }

    pub fn shutdown(&self) {
        let (response_tx, response_rx) = bounded(1);
        let _ = self.inner.commands.send(AudioHubCommand::Shutdown {
            response: response_tx,
        });
        let _ = response_rx.recv();
        if let Some(thread) = self.inner.thread.lock().take() {
            let _ = thread.join();
        }
    }
}

impl Default for AudioHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioHubInner {
    fn drop(&mut self) {
        let (response_tx, response_rx) = bounded(1);
        let _ = self.commands.send(AudioHubCommand::Shutdown {
            response: response_tx,
        });
        let _ = response_rx.recv();
        if let Some(thread) = self.thread.get_mut().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AudioSubscription {
    fn drop(&mut self) {
        self.inner.subscribers.lock().remove(&self.id);
        let _ = self.inner.commands.send(AudioHubCommand::StopIfUnused);
    }
}

fn run_audio_hub_owner(receiver: Receiver<AudioHubCommand>, subscribers: Subscribers) {
    let mut active: Option<(AudioConfig, AudioStream)> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            AudioHubCommand::Ensure { config, response } => {
                let result = match active.as_ref().map(|(current, _)| current) {
                    Some(current) if current == &config => Ok(()),
                    Some(_) if subscribers.lock().len() > 1 => Err(WakeWordError::Audio(
                        "wake word and dictation requested different audio configurations".into(),
                    )),
                    Some(_) => {
                        drop(active.take());
                        start_hub_stream(&config, Arc::clone(&subscribers))
                            .map(|stream| active = Some((config, stream)))
                    }
                    None => start_hub_stream(&config, Arc::clone(&subscribers))
                        .map(|stream| active = Some((config, stream))),
                };
                let _ = response.send(result);
            }
            AudioHubCommand::StopIfUnused => {
                if subscribers.lock().is_empty() {
                    drop(active.take());
                }
            }
            AudioHubCommand::Shutdown { response } => {
                drop(active.take());
                let _ = response.send(());
                break;
            }
        }
    }
}

fn start_hub_stream(config: &AudioConfig, subscribers: Subscribers) -> WakeWordResult<AudioStream> {
    let sample_rate = config.sample_rate;
    AudioStream::start(config.device_id.as_deref(), sample_rate, move |frames| {
        let subscribers = subscribers.lock();
        for subscriber in subscribers.values() {
            subscriber(frames);
        }
    })
}

impl AudioStream {
    /// Start capturing audio and call `on_frames` with mono i16 frames.
    pub fn start<F>(
        device_id: Option<&str>,
        target_sample_rate: u32,
        mut on_frames: F,
    ) -> WakeWordResult<Self>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        let host = cpal::default_host();

        let device = match device_id {
            Some(id) => host
                .input_devices()
                .map_err(WakeWordError::from)?
                .find(|d| d.name().ok().as_deref() == Some(id))
                .ok_or_else(|| WakeWordError::Audio(format!("device not found: {id}")))?,
            None => host
                .default_input_device()
                .ok_or_else(|| WakeWordError::Audio("no default input device".into()))?,
        };

        let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
        tracing::info!("fono-wake audio: using device «{device_name}»");

        let supported: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| WakeWordError::Audio(e.to_string()))?
            .collect();

        let range = supported
            .iter()
            .min_by_key(|config| {
                let min_rate = config.min_sample_rate().0;
                let max_rate = config.max_sample_rate().0;
                let rate_distance =
                    if min_rate <= target_sample_rate && target_sample_rate <= max_rate {
                        0
                    } else {
                        min_rate
                            .abs_diff(target_sample_rate)
                            .min(max_rate.abs_diff(target_sample_rate))
                    };
                (
                    if config.channels() == 1 { 0 } else { 1 },
                    rate_distance,
                    sample_format_priority(config.sample_format()),
                )
            })
            .ok_or_else(|| WakeWordError::Audio("no supported input config".into()))?;

        let target_rate = SampleRate(target_sample_rate);
        let sample_rate =
            if range.min_sample_rate() <= target_rate && target_rate <= range.max_sample_rate() {
                target_rate
            } else {
                range.max_sample_rate().min(SampleRate(48_000))
            };

        let supported_config = (*range).with_sample_rate(sample_rate);
        let channels = supported_config.channels();
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();
        let in_rate = stream_config.sample_rate.0 as f32;

        tracing::debug!("fono-wake audio: {in_rate} Hz, {channels} ch, {sample_format:?}");

        // These buffers live with the callback and are reused for every audio
        // frame. Normal device chunks therefore do not allocate on the real-
        // time path after the stream has started.
        let mut converted = Vec::with_capacity(8_192);
        let mut mono = Vec::with_capacity(8_192);
        let mut resampled = Vec::with_capacity(8_192);
        let needs_resample = (in_rate - target_sample_rate as f32).abs() > 1.0;

        let stream = device.build_input_stream_raw(
            &stream_config,
            sample_format,
            move |data: &cpal::Data, _: &_| {
                convert_to_i16(data, sample_format, &mut converted);
                if channels <= 1 {
                    if needs_resample {
                        linear_resample(
                            &converted,
                            in_rate,
                            target_sample_rate as f32,
                            &mut resampled,
                        );
                        on_frames(&resampled);
                    } else {
                        on_frames(&converted);
                    }
                } else {
                    mix_to_mono(&converted, channels as usize, &mut mono);
                    if needs_resample {
                        linear_resample(&mono, in_rate, target_sample_rate as f32, &mut resampled);
                        on_frames(&resampled);
                    } else {
                        on_frames(&mono);
                    }
                }
            },
            |err| tracing::error!("fono-wake audio stream error: {err}"),
            None,
        )?;

        stream.play()?;
        Ok(Self { _stream: stream })
    }
}

fn sample_format_priority(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::F64 => 3,
        SampleFormat::I64 => 4,
        SampleFormat::U16 => 5,
        SampleFormat::U32 => 6,
        SampleFormat::U64 => 7,
        SampleFormat::I8 => 8,
        SampleFormat::U8 => 9,
        _ => 10,
    }
}

fn convert_to_i16(data: &cpal::Data, format: SampleFormat, output: &mut Vec<i16>) {
    output.clear();
    let bytes = data.bytes();
    match format {
        SampleFormat::I8 => samples_to_i16::<i8, _>(bytes, output, |s| (s as i16) << 8),
        SampleFormat::I16 => samples_to_i16::<i16, _>(bytes, output, |s| s),
        SampleFormat::I32 => samples_to_i16::<i32, _>(bytes, output, |s| (s >> 16) as i16),
        SampleFormat::I64 => samples_to_i16::<i64, _>(bytes, output, |s| (s >> 48) as i16),
        SampleFormat::U8 => samples_to_i16::<u8, _>(bytes, output, |s| (s as i16 - 128) << 8),
        SampleFormat::U16 => samples_to_i16::<u16, _>(bytes, output, |s| (s as i32 - 32768) as i16),
        SampleFormat::U32 => {
            samples_to_i16::<u32, _>(bytes, output, |s| ((s as i64 - 2_147_483_648) >> 16) as i16)
        }
        SampleFormat::U64 => samples_to_i16::<u64, _>(bytes, output, |s| {
            ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16
        }),
        SampleFormat::F32 => samples_to_i16::<f32, _>(bytes, output, |s| float_to_i16(s as f64)),
        SampleFormat::F64 => samples_to_i16::<f64, _>(bytes, output, float_to_i16),
        _ => {
            tracing::warn!("fono-wake: unsupported sample format {format:?}");
        }
    }
}

fn samples_to_i16<T: Copy, F>(bytes: &[u8], output: &mut Vec<i16>, mut convert: F)
where
    F: FnMut(T) -> i16,
{
    if bytes.is_empty() {
        return;
    }
    let sample_size = std::mem::size_of::<T>();
    if bytes.len() % sample_size != 0 {
        tracing::warn!("fono-wake: misaligned audio bytes");
    }
    // SAFETY: `align_to` never reinterprets the unaligned prefix/suffix. The
    // aligned middle is read only as `T: Copy`; the caller selects `T` from
    // CPAL's declared sample format, so no references outlive `bytes`.
    let (_, samples, _) = unsafe { bytes.align_to::<T>() };
    output.extend(samples.iter().copied().map(&mut convert));
}

fn float_to_i16(v: f64) -> i16 {
    (v.clamp(-1.0, 1.0) * i16::MAX as f64) as i16
}

fn mix_to_mono(data: &[i16], channels: usize, output: &mut Vec<i16>) {
    output.clear();
    output.extend(data.chunks(channels).map(|chunk| {
        let sum: i64 = chunk.iter().map(|&s| s as i64).sum();
        (sum / chunk.len() as i64) as i16
    }));
}

fn linear_resample(input: &[i16], in_rate: f32, out_rate: f32, output: &mut Vec<i16>) {
    output.clear();
    if input.is_empty() {
        return;
    }
    let ratio = out_rate / in_rate;
    let out_len = ((input.len() as f32) * ratio) as usize;
    let last = input.len().saturating_sub(1);
    output.extend((0..out_len).map(|i| {
        let src = i as f32 / ratio;
        let idx = src.floor() as usize;
        let frac = src - idx as f32;
        let a = input[idx.min(last)] as f32;
        let b = input[(idx + 1).min(last)] as f32;
        (a + (b - a) * frac) as i16
    }));
}
