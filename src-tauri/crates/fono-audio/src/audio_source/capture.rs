use super::{
    convert::{convert_to_i16, mix_to_mono, sample_format_priority},
    resample::StreamingResampler,
};
use crate::error::{AudioError, AudioResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate};
use crossbeam_channel::bounded;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POOL_SIZE: usize = 32;
const MAX_INTERLEAVED_SAMPLES: usize = 65_536;
// The preallocated pool covers 32 callbacks without enlarging the RT queue.
// Brief scheduler stalls may lose a packet; repeated loss is a capture fault.
const CAPTURE_LOSS_WINDOW: Duration = Duration::from_secs(2);
const MAX_TOLERATED_CAPTURE_LOSSES: u64 = 8;

struct CaptureLossMonitor {
    observed: u64,
    recent: VecDeque<(Duration, u64)>,
    recent_dropped: u64,
}

impl CaptureLossMonitor {
    fn new(observed: u64) -> Self {
        Self {
            observed,
            recent: VecDeque::with_capacity(MAX_TOLERATED_CAPTURE_LOSSES as usize),
            recent_dropped: 0,
        }
    }

    fn observe(&mut self, total_dropped: u64, elapsed: Duration) -> (u64, u64) {
        while let Some(&(at, count)) = self.recent.front() {
            if elapsed.saturating_sub(at) < CAPTURE_LOSS_WINDOW {
                break;
            }
            self.recent.pop_front();
            self.recent_dropped -= count;
        }
        let newly_dropped = super::capture_loss_delta(self.observed, total_dropped).unwrap_or(0);
        self.observed = total_dropped;
        self.recent_dropped = self.recent_dropped.saturating_add(newly_dropped);
        // At most eight non-empty observations can survive before failure.
        if newly_dropped > 0 && self.recent_dropped <= MAX_TOLERATED_CAPTURE_LOSSES {
            self.recent.push_back((elapsed, newly_dropped));
        }
        (newly_dropped, self.recent_dropped)
    }
}

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
    ) -> AudioResult<Self>
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
    ) -> AudioResult<Self>
    where
        F: FnMut(&[i16]) + Send + 'static,
        E: FnMut(String) + Send + 'static,
    {
        if target_sample_rate == 0 {
            return Err(AudioError::Audio("zero sample rate".into()));
        }
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) => host
                .input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(id))
                .ok_or_else(|| AudioError::Audio(format!("device not found: {id}")))?,
            None => host
                .default_input_device()
                .ok_or_else(|| AudioError::Audio("no default input device".into()))?,
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
            .ok_or_else(|| AudioError::Audio("no supported input config".into()))?;
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
            let started = Instant::now();
            let mut losses = CaptureLossMonitor::new(initial_dropped);
            let mut mono = Vec::with_capacity(MAX_INTERLEAVED_SAMPLES);
            let mut output = Vec::new();
            let mut resampler = StreamingResampler::new(rate, target_sample_rate);
            while !stop.load(Ordering::Acquire) {
                let (newly_dropped, recent_dropped) =
                    losses.observe(worker_dropped.load(Ordering::Relaxed), started.elapsed());
                if recent_dropped > MAX_TOLERATED_CAPTURE_LOSSES {
                    on_error(format!(
                        "audio capture overflow: {recent_dropped} input packets lost within 2 seconds"
                    ));
                    break;
                }
                if newly_dropped > 0 {
                    tracing::warn!(
                        newly_dropped,
                        recent_dropped,
                        "audio capture degraded: input packets were lost"
                    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_drop_is_counted_without_stopping_capture() {
        let dropped = AtomicU64::new(12);
        let mut losses = CaptureLossMonitor::new(dropped.load(Ordering::Relaxed));
        dropped.fetch_add(1, Ordering::Relaxed);
        let (newly_dropped, recent) =
            losses.observe(dropped.load(Ordering::Relaxed), Duration::from_millis(20));
        assert_eq!(newly_dropped, 1);
        assert_eq!(dropped.load(Ordering::Relaxed), 13);
        assert!(recent <= MAX_TOLERATED_CAPTURE_LOSSES);
        assert_eq!(losses.observe(13, Duration::from_millis(40)), (0, 1));
        assert_eq!(losses.observe(13, Duration::from_secs(3)), (0, 0));
    }

    #[test]
    fn sustained_packet_loss_faults_capture() {
        let mut losses = CaptureLossMonitor::new(0);
        for count in 1..=MAX_TOLERATED_CAPTURE_LOSSES {
            let (_, recent) = losses.observe(count, Duration::from_millis(count * 100));
            assert!(recent <= MAX_TOLERATED_CAPTURE_LOSSES);
        }
        let (_, recent) = losses.observe(9, Duration::from_millis(900));
        assert!(recent > MAX_TOLERATED_CAPTURE_LOSSES);
    }

    #[test]
    fn separated_loss_bursts_do_not_accumulate_forever() {
        let mut losses = CaptureLossMonitor::new(0);
        assert_eq!(losses.observe(8, Duration::ZERO), (8, 8));
        assert_eq!(losses.observe(9, CAPTURE_LOSS_WINDOW), (1, 1));
        assert!(losses.recent.len() <= MAX_TOLERATED_CAPTURE_LOSSES as usize);
    }

    #[test]
    fn a_large_loss_observed_after_worker_stall_faults_immediately() {
        let mut losses = CaptureLossMonitor::new(10);
        assert_eq!(losses.observe(110, Duration::from_secs(5)), (100, 100));
        assert!(losses.recent.is_empty());
    }

    #[test]
    fn cumulative_counter_wrap_is_counted_and_reset_is_rebased() {
        let mut wrapped = CaptureLossMonitor::new(u64::MAX - 1);
        assert_eq!(wrapped.observe(1, Duration::ZERO), (3, 3));
        let mut reset = CaptureLossMonitor::new(100);
        assert_eq!(reset.observe(0, Duration::ZERO), (0, 0));
        assert_eq!(reset.observe(1, Duration::from_millis(20)), (1, 1));
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
