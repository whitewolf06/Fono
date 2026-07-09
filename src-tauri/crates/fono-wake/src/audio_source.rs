//! Minimal CPAL-based audio capture used by wake word backends.
//!
//! Captures mono i16 samples at the requested sample rate (default 16 kHz).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};

use crate::error::{WakeWordError, WakeWordResult};

pub struct AudioStream {
    _stream: cpal::Stream,
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
            .find(|c| c.channels() == 1 && c.min_sample_rate().0 <= target_sample_rate)
            .or_else(|| supported.first())
            .ok_or_else(|| WakeWordError::Audio("no supported input config".into()))?;

        let target_rate = SampleRate(target_sample_rate);
        let sample_rate = if range.min_sample_rate() <= target_rate && target_rate <= range.max_sample_rate() {
            target_rate
        } else {
            range.max_sample_rate().min(SampleRate(48_000))
        };

        let supported_config = range.clone().with_sample_rate(sample_rate);
        let channels = supported_config.channels();
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();
        let in_rate = stream_config.sample_rate.0 as f32;

        tracing::debug!(
            "fono-wake audio: {in_rate} Hz, {channels} ch, {sample_format:?}"
        );

        let stream = device.build_input_stream_raw(
            &stream_config,
            sample_format,
            move |data: &cpal::Data, _: &_| {
                let i16_data = convert_to_i16(data, sample_format);
                let mono = mix_to_mono(&i16_data, channels as usize);
                let resampled = if (in_rate - target_sample_rate as f32).abs() > 1.0 {
                    linear_resample(&mono, in_rate, target_sample_rate as f32)
                } else {
                    mono
                };
                on_frames(&resampled);
            },
            |err| tracing::error!("fono-wake audio stream error: {err}"),
            None,
        )?;

        stream.play()?;
        Ok(Self { _stream: stream })
    }
}

fn convert_to_i16(data: &cpal::Data, format: SampleFormat) -> Vec<i16> {
    let bytes = data.bytes();
    match format {
        SampleFormat::I8 => samples_to_i16::<i8, _>(bytes, |s| (s as i16) << 8),
        SampleFormat::I16 => samples_to_i16::<i16, _>(bytes, |s| s),
        SampleFormat::I32 => samples_to_i16::<i32, _>(bytes, |s| (s >> 16) as i16),
        SampleFormat::I64 => samples_to_i16::<i64, _>(bytes, |s| (s >> 48) as i16),
        SampleFormat::U8 => samples_to_i16::<u8, _>(bytes, |s| (s as i16 - 128) << 8),
        SampleFormat::U16 => samples_to_i16::<u16, _>(bytes, |s| (s as i32 - 32768) as i16),
        SampleFormat::U32 => samples_to_i16::<u32, _>(bytes, |s| ((s as i64 - 2_147_483_648) >> 16) as i16),
        SampleFormat::U64 => samples_to_i16::<u64, _>(bytes, |s| {
            ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16
        }),
        SampleFormat::F32 => samples_to_i16::<f32, _>(bytes, |s| float_to_i16(s as f64)),
        SampleFormat::F64 => samples_to_i16::<f64, _>(bytes, float_to_i16),
        _ => {
            tracing::warn!("fono-wake: unsupported sample format {format:?}");
            Vec::new()
        }
    }
}

fn samples_to_i16<T: Copy, F>(bytes: &[u8], mut convert: F) -> Vec<i16>
where
    F: FnMut(T) -> i16,
{
    if bytes.is_empty() {
        return Vec::new();
    }
    let sample_size = std::mem::size_of::<T>();
    if bytes.len() % sample_size != 0 {
        tracing::warn!("fono-wake: misaligned audio bytes");
    }
    let (_, samples, _) = unsafe { bytes.align_to::<T>() };
    samples.iter().copied().map(&mut convert).collect()
}

fn float_to_i16(v: f64) -> i16 {
    (v.clamp(-1.0, 1.0) * i16::MAX as f64) as i16
}

fn mix_to_mono(data: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|chunk| {
            let sum: i64 = chunk.iter().map(|&s| s as i64).sum();
            (sum / chunk.len() as i64) as i16
        })
        .collect()
}

fn linear_resample(input: &[i16], in_rate: f32, out_rate: f32) -> Vec<i16> {
    let ratio = out_rate / in_rate;
    let out_len = ((input.len() as f32) * ratio) as usize;
    let last = input.len().saturating_sub(1);
    (0..out_len)
        .map(|i| {
            let src = i as f32 / ratio;
            let idx = src.floor() as usize;
            let frac = src - idx as f32;
            let a = input[idx.min(last)] as f32;
            let b = input[(idx + 1).min(last)] as f32;
            (a + (b - a) * frac) as i16
        })
        .collect()
}
