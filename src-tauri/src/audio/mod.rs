//! Audio capture wrapper using CPAL input stream.
//!
//! Provides a simple API to enumerate input devices and start capture.
//! Captured samples are converted to mono `i16` at 16kHz for Whisper.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};

use crate::error::{AppError, AppResult};
use crate::types::DeviceInfo;

pub struct AudioCapture;

impl AudioCapture {
    /// List available input devices.
    pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().and_then(|d| d.name().ok());

        let mut out = Vec::new();
        if let Ok(iter) = host.input_devices() {
            for dev in iter {
                if let Ok(name) = dev.name() {
                    let is_default = default_name.as_deref() == Some(&name);
                    out.push(DeviceInfo {
                        id: name.clone(),
                        name,
                        is_default,
                    });
                }
            }
        }

        Ok(out)
    }

    /// Start capture with preferred format (default: mono, 16kHz, i16 for callback).
    pub fn start<F>(device_id: Option<&str>, on_samples: F) -> AppResult<cpal::Stream>
    where
        F: Fn(&[i16]) + Send + 'static,
    {
        tracing::info!("AudioCapture::start: device_id={:?}", device_id);

        let host = cpal::default_host();
        tracing::info!("AudioCapture::start: cpal host = {:?}", host.id());

        let device = match device_id {
            Some(id) => {
                let mut iter = host
                    .input_devices()
                    .map_err(|e| {
                        tracing::error!("AudioCapture::start: input_devices failed: {e}");
                        AppError::Audio(format!("input_devices: {e}"))
                    })?;
                iter.find(|d| d.name().ok().as_deref() == Some(id))
                    .ok_or_else(|| {
                        tracing::error!("AudioCapture::start: Device not found: {id}");
                        AppError::Audio(format!("Device not found: {id}"))
                    })?
            }
            None => {
                let d = host.default_input_device().ok_or_else(|| {
                    tracing::error!("AudioCapture::start: no default device");
                    AppError::Audio("No default input device".into())
                })?;
                d
            }
        };

        let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
        tracing::info!("AudioCapture::start: selected device «{device_name}»");

        let supported: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| {
                tracing::error!("AudioCapture::start: supported_input_configs failed: {e}");
                AppError::Audio(format!("supported_input_configs: {e}"))
            })?
            .collect();

        // Prefer mono input and a supported 16kHz rate when possible.
        let range = supported
            .iter()
            .find(|c| c.channels() == 1 && c.min_sample_rate().0 <= 16_000)
            .or_else(|| supported.first())
            .ok_or_else(|| AppError::Audio("No supported input config".into()))?;

        let target_rate = SampleRate(16_000);
        let sample_rate = if range.min_sample_rate() <= target_rate && target_rate <= range.max_sample_rate()
        {
            target_rate
        } else {
            range.max_sample_rate().min(SampleRate(48_000))
        };

        let supported_config = range.clone().with_sample_rate(sample_rate);
        let channels = supported_config.channels();
        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();

        tracing::info!(
            "AudioCapture::start: config = {}Hz, {}ch, {:?}",
            stream_config.sample_rate.0,
            channels,
            sample_format
        );

        let in_rate = stream_config.sample_rate.0 as f32;

        let stream = device
            .build_input_stream_raw(
                &stream_config,
                sample_format,
                move |data: &cpal::Data, _: &_| {
                    let i16_data = convert_chunk_to_i16(data, sample_format);
                    let mono = mix_to_mono_i16(&i16_data, channels as usize);
                    let resampled = linear_resample(&mono, in_rate, 16_000.0);
                    on_samples(&resampled);
                },
                |err| tracing::error!("audio stream error: {err}"),
                None,
            )
            .map_err(|e| {
                tracing::error!("AudioCapture::start: build_input_stream FAILED: {e}");
                AppError::Audio(format!("build_input_stream: {e}"))
            })?;
        tracing::info!("AudioCapture::start: build_input_stream OK");

        stream
            .play()
            .map_err(|e| {
                tracing::error!("AudioCapture::start: stream.play FAILED: {e}");
                AppError::Audio(format!("stream.play: {e}"))
            })?;
        tracing::info!("AudioCapture::start: stream.play OK — capture active");

        tracing::info!(
            "audio capture started: device «{device_name}» {}Hz {}ch {:?}",
            in_rate,
            channels,
            sample_format
        );

        Ok(stream)
    }
}

/// Mix multi-channel samples into mono i16 by averaging.
fn mix_to_mono_i16(data: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.to_vec();
    }

    let mut out = Vec::with_capacity(data.len() / channels);
    for chunk in data.chunks(channels) {
        let sum: i64 = chunk.iter().map(|&s| s as i64).sum();
        out.push((sum / chunk.len() as i64) as i16);
    }
    out
}

fn convert_chunk_to_i16(data: &cpal::Data, format: SampleFormat) -> Vec<i16> {
    let bytes = data.bytes();
    match format {
        SampleFormat::I8 => samples_to_i16::<i8, _>(bytes, |s| (s as i16) << 8),
        SampleFormat::I16 => samples_to_i16::<i16, _>(bytes, |s| s),
        SampleFormat::I32 => samples_to_i16::<i32, _>(bytes, |s| (s >> 16) as i16),
        SampleFormat::I64 => samples_to_i16::<i64, _>(bytes, |s| (s >> 48) as i16),
        SampleFormat::U8 => samples_to_i16::<u8, _>(bytes, |s| (s as i16 - 128) << 8),
        SampleFormat::U16 => samples_to_i16::<u16, _>(bytes, |s| (s as i32 - 32768) as i16),
        SampleFormat::U32 => {
            samples_to_i16::<u32, _>(bytes, |s| ((s as i64 - 2_147_483_648) >> 16) as i16)
        }
        SampleFormat::U64 => {
            samples_to_i16::<u64, _>(bytes, |s| {
                ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16
            })
        }
        SampleFormat::F32 => samples_to_i16::<f32, _>(bytes, |s| float_to_i16(s as f64)),
        SampleFormat::F64 => samples_to_i16::<f64, _>(bytes, float_to_i16),
        other => {
            tracing::warn!("Unknown sample format: {other:?}");
            Vec::new()
        }
    }
}

fn samples_to_i16<T: Copy, F>(bytes: &[u8], convert: F) -> Vec<i16>
where
    F: FnMut(T) -> i16,
{
    if bytes.is_empty() {
        return Vec::new();
    }

    let sample_size = std::mem::size_of::<T>();
    if bytes.len() % sample_size != 0 {
        tracing::warn!(
            "audio bytes length {} is not aligned to sample size {}",
            bytes.len(),
            sample_size
        );
    }

    let (_, samples, _) = unsafe { bytes.align_to::<T>() };
    let mut f = convert;
    samples.iter().copied().map(&mut f).collect()
}

fn float_to_i16(v: f64) -> i16 {
    let clamped = v.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f64) as i16
}

/// Simple linear resampler from any integer sample rate to target rate.
fn linear_resample(input: &[i16], in_rate: f32, out_rate: f32) -> Vec<i16> {
    if (in_rate - out_rate).abs() < 1.0 {
        return input.to_vec();
    }

    let ratio = out_rate / in_rate;
    let out_len = ((input.len() as f32) * ratio) as usize;
    let last = input.len().saturating_sub(1);
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = (i as f32) / ratio;
        let idx = src.floor() as usize;
        let frac = src - idx as f32;
        let a = input[idx.min(last)] as f32;
        let b = input[(idx + 1).min(last)] as f32;
        out.push((a + (b - a) * frac) as i16);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes_from_i8(samples: &[i8]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn bytes_from_i16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_i32(samples: &[i32]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_i64(samples: &[i64]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_u8(samples: &[u8]) -> Vec<u8> {
        samples.to_vec()
    }

    fn bytes_from_u16(samples: &[u16]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_u32(samples: &[u32]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_u64(samples: &[u64]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_f32(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn bytes_from_f64(samples: &[f64]) -> Vec<u8> {
        samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn sample_to_i16_i8() {
        let raw = bytes_from_i8(&[-128, -1, 0, 1, 127]);
        let out = samples_to_i16::<i8, _>(&raw, |s| (s as i16) << 8);
        assert_eq!(out, vec![-32768, -256, 0, 256, 32512]);
    }

    #[test]
    fn sample_to_i16_i16() {
        let raw = bytes_from_i16(&[-32_768, -1, 0, 1, 32_767]);
        let out = samples_to_i16::<i16, _>(&raw, |s| s);
        assert_eq!(out, vec![-32_768, -1, 0, 1, 32_767]);
    }

    #[test]
    fn sample_to_i16_i32() {
        let raw = bytes_from_i32(&[i32::MIN, -65536, 0, 65536, i32::MAX]);
        let out = samples_to_i16::<i32, _>(&raw, |s| (s >> 16) as i16);
        assert_eq!(out, vec![-32768, -1, 0, 1, 32767]);
    }

    #[test]
    fn sample_to_i16_i64() {
        let raw = bytes_from_i64(&[i64::MIN, 0, 1, i64::MAX]);
        let out = samples_to_i16::<i64, _>(&raw, |s| (s >> 48) as i16);
        assert_eq!(out, vec![-32768, 0, 0, 32767]);
    }

    #[test]
    fn sample_to_i16_u8() {
        let raw = bytes_from_u8(&[0, 1, 127, 128, 255]);
        let out = samples_to_i16::<u8, _>(&raw, |s| (s as i16 - 128) << 8);
        assert_eq!(out, vec![-32768, -32512, -256, 0, 32512]);
    }

    #[test]
    fn sample_to_i16_u16() {
        let raw = bytes_from_u16(&[0, 1, 32767, 32768, 65535]);
        let out = samples_to_i16::<u16, _>(&raw, |s| (s as i32 - 32768) as i16);
        assert_eq!(out, vec![-32768, -32767, -1, 0, 32767]);
    }

    #[test]
    fn sample_to_i16_u32() {
        let raw = bytes_from_u32(&[0, 1, 2_147_483_647, 2_147_483_648, 4_294_967_295]);
        let out = samples_to_i16::<u32, _>(
            &raw,
            |s| ((s as i64 - 2_147_483_648) >> 16) as i16,
        );
        assert_eq!(out, vec![-32768, -32768, -1, 0, 32767]);
    }

    #[test]
    fn sample_to_i16_u64() {
        let raw = bytes_from_u64(&[0, 1, 9_223_372_036_854_775_808, u64::MAX]);
        let out = samples_to_i16::<u64, _>(
            &raw,
            |s| ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16,
        );
        assert_eq!(out, vec![-32768, -32768, 0, 32767]);
    }

    #[test]
    fn sample_to_i16_f32() {
        let raw = bytes_from_f32(&[-1.0, -0.5, 0.0, 0.5, 1.0]);
        let out = samples_to_i16::<f32, _>(&raw, |s| float_to_i16(s as f64));
        assert_eq!(
            out,
            vec![
                -32767,
                (-0.5f64 * i16::MAX as f64) as i16,
                0,
                (0.5f64 * i16::MAX as f64) as i16,
                32767
            ]
        );
    }

    #[test]
    fn sample_to_i16_f64() {
        let raw = bytes_from_f64(&[-1.0, -0.2, 0.2, 1.2, -1.2]);
        let out = samples_to_i16::<f64, _>(&raw, float_to_i16);
        assert_eq!(
            out,
            vec![
                -32767,
                (-0.2f64 * i16::MAX as f64) as i16,
                (0.2f64 * i16::MAX as f64) as i16,
                32767,
                -32767
            ]
        );
    }
}
