//! Bounded technical telemetry. No API accepts transcripts, paths, URLs or errors.

use std::fmt::Write as _;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fono_stt::scheduler::QueueClass;
use serde::Serialize;

use crate::config::LogLevel;

const TIME_BUCKETS: &[f64] = &[0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 15.0, 60.0, 300.0];
const RTF_BUCKETS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
const STATUSES: &[u16] = &[
    200, 202, 204, 400, 401, 403, 404, 405, 408, 413, 415, 422, 429, 500, 502, 503, 504,
];

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Listening,
    ModelReady,
    ModelFailed,
    Stopping,
    Stopped,
    Failed,
    ConfigurationFailed,
    TokensFailed,
    BindFailed,
    UploadFailed,
    SignalFailed,
    ShutdownTimedOut,
    NetworkExposure,
    TlsReloaded,
    TlsReloadRejected,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Preempted,
    Cancelled,
    WorkerFailed,
    Failed,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Preempted => "preempted",
            Self::Cancelled => "cancelled",
            Self::WorkerFailed => "worker_failed",
            Self::Failed => "failed",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    Lifecycle {
        phase: Phase,
    },
    HttpCompleted {
        status: u16,
        elapsed_us: u64,
    },
    InferenceWindow {
        outcome: Outcome,
        elapsed_us: u64,
        audio_ms: u64,
    },
}

#[derive(Serialize)]
struct Record {
    timestamp: String,
    level: LogLevel,
    #[serde(flatten)]
    event: Event,
}

struct Histogram {
    bounds: &'static [f64],
    data: Mutex<(Vec<u64>, u64, f64)>,
}
impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        Self {
            bounds,
            data: Mutex::new((vec![0; bounds.len()], 0, 0.0)),
        }
    }
    fn observe(&self, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        for (i, bound) in self.bounds.iter().enumerate() {
            if value <= *bound {
                data.0[i] += 1;
            }
        }
        data.1 += 1;
        data.2 += value;
    }
    fn render(&self, name: &str, output: &mut String) {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(output, "# TYPE {name} histogram");
        for (bound, count) in self.bounds.iter().zip(&data.0) {
            let _ = writeln!(output, "{name}_bucket{{le=\"{bound}\"}} {count}");
        }
        let _ = writeln!(
            output,
            "{name}_bucket{{le=\"+Inf\"}} {}\n{name}_sum {:.9}\n{name}_count {}",
            data.1, data.2, data.1
        );
    }
}

pub struct Observability {
    level: LogLevel,
    sender: Mutex<Option<mpsc::SyncSender<Record>>>,
    dropped: Arc<AtomicU64>,
    http: Histogram,
    wait: Histogram,
    decode: Histogram,
    acquire: Histogram,
    audio_decode: Histogram,
    rtf: Histogram,
    statuses: [AtomicU64; 18],
    outcomes: [AtomicU64; 5],
    waiting: [AtomicU64; 2],
    active: [AtomicU64; 2],
    gpu: Mutex<Option<Vec<(u32, u64, u64)>>>,
}

impl Default for Observability {
    fn default() -> Self {
        Self {
            level: LogLevel::Off,
            sender: Mutex::new(None),
            dropped: Arc::new(AtomicU64::new(0)),
            http: Histogram::new(TIME_BUCKETS),
            wait: Histogram::new(TIME_BUCKETS),
            decode: Histogram::new(TIME_BUCKETS),
            acquire: Histogram::new(TIME_BUCKETS),
            audio_decode: Histogram::new(TIME_BUCKETS),
            rtf: Histogram::new(RTF_BUCKETS),
            statuses: std::array::from_fn(|_| AtomicU64::new(0)),
            outcomes: std::array::from_fn(|_| AtomicU64::new(0)),
            waiting: std::array::from_fn(|_| AtomicU64::new(0)),
            active: std::array::from_fn(|_| AtomicU64::new(0)),
            gpu: Mutex::new(None),
        }
    }
}

pub struct LogGuard {
    owner: Arc<Observability>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for LogGuard {
    fn drop(&mut self) {
        self.owner
            .sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }
}
impl LogGuard {
    /// Closing admission wakes the writer after it drains its bounded queue.
    /// A blocked stdout reader cannot extend the server shutdown indefinitely.
    pub fn finish(mut self) {
        self.owner
            .sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(thread) = self.thread.take() {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

impl Observability {
    pub fn stdout(level: LogLevel) -> std::io::Result<(Arc<Self>, LogGuard)> {
        Self::with_writer(level, std::io::stdout())
    }
    pub(crate) fn with_writer(
        level: LogLevel,
        mut writer: impl Write + Send + 'static,
    ) -> std::io::Result<(Arc<Self>, LogGuard)> {
        let (sender, receiver) = mpsc::sync_channel::<Record>(256);
        let owner = Arc::new(Self {
            level,
            sender: Mutex::new(Some(sender)),
            ..Self::default()
        });
        let dropped = Arc::clone(&owner.dropped);
        let thread = std::thread::Builder::new()
            .name("fono-json-log".into())
            .spawn(move || {
                while let Ok(record) = receiver.recv() {
                    let mut line =
                        serde_json::to_vec(&record).expect("typed log record is serializable");
                    line.push(b'\n');
                    if writer
                        .write_all(&line)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })?;
        Ok((
            owner.clone(),
            LogGuard {
                owner,
                thread: Some(thread),
            },
        ))
    }
    fn emit(&self, level: LogLevel, event: Event) {
        if level > self.level || self.level == LogLevel::Off {
            return;
        }
        let record = Record {
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level,
            event,
        };
        let sender = self.sender.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(sender) = sender.as_ref() {
            if sender.try_send(record).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub fn event(&self, level: LogLevel, phase: Phase) {
        self.emit(level, Event::Lifecycle { phase });
    }
    pub fn http_completed(&self, status: u16, elapsed: Duration) {
        self.http.observe(elapsed.as_secs_f64());
        let index = STATUSES
            .iter()
            .position(|value| *value == status)
            .unwrap_or(STATUSES.len());
        self.statuses[index].fetch_add(1, Ordering::Relaxed);
        self.emit(
            if status >= 500 {
                LogLevel::Error
            } else if status >= 400 {
                LogLevel::Warn
            } else {
                LogLevel::Debug
            },
            Event::HttpCompleted {
                status,
                elapsed_us: micros(elapsed),
            },
        );
    }
    pub fn model_acquired(&self, elapsed: Duration) {
        self.acquire.observe(elapsed.as_secs_f64());
    }
    pub fn audio_decoded(&self, elapsed: Duration) {
        self.audio_decode.observe(elapsed.as_secs_f64());
    }
    pub fn waiting(&self, class: QueueClass) -> QueueObservation<'_> {
        let index = priority_index(class);
        self.waiting[index].fetch_add(1, Ordering::Relaxed);
        QueueObservation {
            owner: self,
            index,
            started: Instant::now(),
        }
    }
    pub fn active(&self, class: QueueClass) -> ActiveObservation<'_> {
        let index = priority_index(class);
        self.active[index].fetch_add(1, Ordering::Relaxed);
        ActiveObservation { owner: self, index }
    }
    pub fn inference(&self, outcome: Outcome, elapsed: Duration, samples: usize) {
        self.decode.observe(elapsed.as_secs_f64());
        self.outcomes[outcome.index()].fetch_add(1, Ordering::Relaxed);
        if matches!(outcome, Outcome::Ok) && samples > 0 {
            self.rtf
                .observe(elapsed.as_secs_f64() / (samples as f64 / 16_000.0));
        }
        self.emit(
            if matches!(outcome, Outcome::WorkerFailed | Outcome::Failed) {
                LogLevel::Error
            } else {
                LogLevel::Debug
            },
            Event::InferenceWindow {
                outcome,
                elapsed_us: micros(elapsed),
                audio_ms: (samples as u64).saturating_mul(1000) / 16_000,
            },
        );
    }
    pub fn observe_gpu(&self, devices: Option<Vec<crate::backends::CudaDevice>>) {
        *self.gpu.lock().unwrap_or_else(|p| p.into_inner()) = devices.and_then(|devices| {
            devices
                .into_iter()
                .map(|device| {
                    Some((
                        device.index,
                        device.memory_total_mb.checked_mul(1024 * 1024)?,
                        device.memory_free_mb.checked_mul(1024 * 1024)?,
                    ))
                })
                .collect()
        });
    }
    pub fn render(&self, output: &mut String) {
        for (name, histogram) in [
            ("fono_http_request_duration_seconds", &self.http),
            ("fono_stt_queue_wait_seconds", &self.wait),
            ("fono_stt_decode_seconds", &self.decode),
            ("fono_model_acquire_seconds", &self.acquire),
            ("fono_audio_decode_seconds", &self.audio_decode),
            ("fono_stt_realtime_factor", &self.rtf),
        ] {
            histogram.render(name, output);
        }
        let _ = writeln!(output, "# TYPE fono_http_responses_total counter");
        for (i, status) in STATUSES.iter().enumerate() {
            let _ = writeln!(
                output,
                "fono_http_responses_total{{status=\"{status}\"}} {}",
                self.statuses[i].load(Ordering::Relaxed)
            );
        }
        let _ = writeln!(output, "fono_http_responses_total{{status=\"other\"}} {}\n# TYPE fono_stt_windows_total counter", self.statuses[STATUSES.len()].load(Ordering::Relaxed));
        for outcome in [
            Outcome::Ok,
            Outcome::Preempted,
            Outcome::Cancelled,
            Outcome::WorkerFailed,
            Outcome::Failed,
        ] {
            let _ = writeln!(
                output,
                "fono_stt_windows_total{{outcome=\"{}\"}} {}",
                outcome.label(),
                self.outcomes[outcome.index()].load(Ordering::Relaxed)
            );
        }
        for (name, values) in [
            ("fono_stt_waiting", &self.waiting),
            ("fono_stt_active", &self.active),
        ] {
            let _ = writeln!(output, "# TYPE {name} gauge");
            for (i, priority) in ["interactive", "batch"].iter().enumerate() {
                let _ = writeln!(
                    output,
                    "{name}{{priority=\"{priority}\"}} {}",
                    values[i].load(Ordering::Relaxed)
                );
            }
        }
        let _ = writeln!(output, "# TYPE fono_log_dropped_total counter\nfono_log_dropped_total {}\n# TYPE fono_gpu_inventory_available gauge", self.dropped.load(Ordering::Relaxed));
        let gpu = self.gpu.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(
            output,
            "fono_gpu_inventory_available {}",
            u8::from(gpu.is_some())
        );
        for (name, position) in [
            ("fono_gpu_memory_total_bytes", 0),
            ("fono_gpu_memory_free_bytes", 1),
        ] {
            let _ = writeln!(output, "# TYPE {name} gauge");
            for (index, total, free) in gpu.iter().flatten() {
                let value = if position == 0 { total } else { free };
                let _ = writeln!(output, "{name}{{device=\"cuda:{index}\"}} {value}");
            }
        }
    }
}
fn micros(elapsed: Duration) -> u64 {
    elapsed.as_micros().min(u128::from(u64::MAX)) as u64
}
fn priority_index(class: QueueClass) -> usize {
    usize::from(class == QueueClass::Batch)
}
pub struct QueueObservation<'a> {
    owner: &'a Observability,
    index: usize,
    started: Instant,
}
impl Drop for QueueObservation<'_> {
    fn drop(&mut self) {
        self.owner.waiting[self.index].fetch_sub(1, Ordering::Relaxed);
        self.owner
            .wait
            .observe(self.started.elapsed().as_secs_f64());
    }
}
pub struct ActiveObservation<'a> {
    owner: &'a Observability,
    index: usize,
}
impl Drop for ActiveObservation<'_> {
    fn drop(&mut self) {
        self.owner.active[self.index].fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
