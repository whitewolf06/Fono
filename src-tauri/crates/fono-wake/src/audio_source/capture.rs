use super::{
    convert::{convert_to_i16, mix_to_mono, sample_format_priority},
    resample::StreamingResampler,
};
use crate::error::{WakeWordError, WakeWordResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};
use crossbeam_channel::bounded;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const POOL_SIZE: usize = 32;
const MAX_INTERLEAVED_SAMPLES: usize = 65_536;

pub struct AudioStream {
    stream: Option<cpal::Stream>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl AudioStream {
    pub fn start<F>(
        device_id: Option<&str>,
        target_sample_rate: u32,
        on_frames: F,
    ) -> WakeWordResult<Self>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        Self::start_with_errors(
            device_id,
            target_sample_rate,
            on_frames,
            |message| tracing::error!(%message,"audio input failed"),
            Arc::new(AtomicU64::new(0)),
        )
    }
    pub(super) fn start_with_errors<F, E>(
        device_id: Option<&str>,
        target_sample_rate: u32,
        mut on_frames: F,
        mut on_error: E,
        dropped: Arc<AtomicU64>,
    ) -> WakeWordResult<Self>
    where
        F: FnMut(&[i16]) + Send + 'static,
        E: FnMut(String) + Send + 'static,
    {
        if target_sample_rate == 0 {
            return Err(WakeWordError::Audio("zero sample rate".into()));
        }
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) => host
                .input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(id))
                .ok_or_else(|| WakeWordError::Audio(format!("device not found: {id}")))?,
            None => host
                .default_input_device()
                .ok_or_else(|| WakeWordError::Audio("no default input device".into()))?,
        };
        let supported: Vec<_> = device.supported_input_configs()?.collect();
        let range = supported
            .iter()
            .min_by_key(|config| {
                let min = config.min_sample_rate().0;
                let max = config.max_sample_rate().0;
                let distance = if min <= target_sample_rate && target_sample_rate <= max {
                    0
                } else {
                    min.abs_diff(target_sample_rate)
                        .min(max.abs_diff(target_sample_rate))
                };
                (
                    usize::from(config.channels() != 1),
                    distance,
                    sample_format_priority(config.sample_format()),
                )
            })
            .ok_or_else(|| WakeWordError::Audio("no supported input config".into()))?;
        let rate = target_sample_rate.clamp(range.min_sample_rate().0, range.max_sample_rate().0);
        let supported_config = (*range).with_sample_rate(SampleRate(rate));
        let channels = supported_config.channels() as usize;
        let format = supported_config.sample_format();
        let config: cpal::StreamConfig = supported_config.into();
        let (pool_tx, pool_rx) = bounded::<Vec<i16>>(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            pool_tx
                .send(Vec::with_capacity(MAX_INTERLEAVED_SAMPLES))
                .unwrap();
        }
        let (raw_tx, raw_rx) = bounded::<Vec<i16>>(POOL_SIZE);
        let (error_tx, error_rx) = bounded::<String>(1);
        let callback_pool = pool_tx.clone();
        let worker_dropped = dropped.clone();
        let stream = device.build_input_stream_raw(
            &config,
            format,
            move |data: &cpal::Data, _| {
                let sample_bytes = match format {
                    SampleFormat::I8 | SampleFormat::U8 => 1,
                    SampleFormat::I16 | SampleFormat::U16 => 2,
                    SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => 4,
                    _ => 8,
                };
                if data.bytes().len() / sample_bytes > MAX_INTERLEAVED_SAMPLES {
                    dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                if let Ok(mut buffer) = pool_rx.try_recv() {
                    convert_to_i16(data, format, &mut buffer);
                    if let Err(error) = raw_tx.try_send(buffer) {
                        dropped.fetch_add(1, Ordering::Relaxed);
                        let _ = callback_pool.try_send(error.into_inner());
                    }
                } else {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            },
            move |error| {
                let _ = error_tx.try_send(error.to_string());
            },
            None,
        )?;
        let initial_dropped = worker_dropped.load(Ordering::Relaxed);
        stream.play()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::spawn(move || {
            let mut mono = Vec::with_capacity(MAX_INTERLEAVED_SAMPLES);
            let mut output = Vec::new();
            let mut resampler = StreamingResampler::new(rate, target_sample_rate);
            while !stop.load(Ordering::Acquire) {
                if worker_dropped.load(Ordering::Relaxed) != initial_dropped {
                    on_error("audio capture overflow: input frames were lost".into());
                    break;
                }
                if let Ok(message) = error_rx.try_recv() {
                    on_error(message);
                    break;
                }
                match raw_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(mut raw) => {
                        let input = if channels == 1 {
                            &raw[..]
                        } else {
                            mix_to_mono(&raw, channels, &mut mono);
                            &mono[..]
                        };
                        resampler.process(input, &mut output);
                        if !output.is_empty() {
                            on_frames(&output);
                        }
                        raw.clear();
                        let _ = pool_tx.try_send(raw);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Ok(Self {
            stream: Some(stream),
            stopped,
            worker: Some(worker),
        })
    }
}
impl Drop for AudioStream {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.stream.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
