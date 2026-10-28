//! Runtime diagnostics for wake word backends.
//!
//! Used by the settings UI to show real-time audio levels and the last
//! spotter result.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;

/// Diagnostic snapshot exposed to the frontend.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Diagnostics {
    pub running: bool,
    pub paused: bool,
    /// Number of audio frames processed since start.
    pub frames_received: u64,
    /// RMS audio level, 0.0..1.0.
    pub rms: f32,
    /// Peak audio level, 0.0..1.0.
    pub peak: f32,
    /// Last event emitted by the backend (e.g. "Listening", "Detected").
    pub last_event: String,
    /// Last non-empty keyword returned by the spotter.
    pub last_result_keyword: String,
    /// Raw JSON/debug text of the last spotter result.
    pub last_result_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsInner {
    pub data: Diagnostics,
}

pub type DiagnosticsHandle = Arc<Mutex<DiagnosticsInner>>;

pub fn new_handle() -> DiagnosticsHandle {
    Arc::new(Mutex::new(DiagnosticsInner::default()))
}

pub fn update_audio_level(handle: &DiagnosticsHandle, samples: &[f32]) {
    if samples.is_empty() {
        return;
    }
    let mut sum_sq: f64 = 0.0;
    let mut peak: f32 = 0.0;
    for &s in samples {
        let d = s as f64;
        sum_sq += d * d;
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }
    let rms = ((sum_sq / samples.len() as f64) as f32).sqrt();
    let mut inner = handle.lock();
    inner.data.frames_received += samples.len() as u64;
    inner.data.rms = rms;
    inner.data.peak = peak;
}

pub fn record_event(handle: &DiagnosticsHandle, event: &str) {
    handle.lock().data.last_event = event.into();
}

pub fn record_result(handle: &DiagnosticsHandle, keyword: &str, json: &str) {
    let mut inner = handle.lock();
    inner.data.last_result_keyword = keyword.into();
    inner.data.last_result_json = json.into();
}

pub fn set_running(handle: &DiagnosticsHandle, running: bool) {
    handle.lock().data.running = running;
}

pub fn set_paused(handle: &DiagnosticsHandle, paused: bool) {
    handle.lock().data.paused = paused;
}
