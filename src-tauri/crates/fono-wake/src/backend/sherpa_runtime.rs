use super::audio_recovery::{wait_while_running, AudioRecovery};
use super::decoder_session::DecoderSession;
use super::sherpa_detector::SherpaDetector;
use crate::{
    callback::CallbackSlot,
    diag::{self, DiagnosticsHandle},
};
use crate::{
    AudioCursor, AudioHub, AudioPacket, WakeWordConfig, WakeWordError, WakeWordEvent,
    WakeWordResult, WakeWordStatus,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
pub(super) struct Runtime {
    pub(super) config: WakeWordConfig,
    pub(super) hub: AudioHub,
    pub(super) status: Arc<Mutex<WakeWordStatus>>,
    pub(super) running: Arc<AtomicBool>,
    pub(super) paused: Arc<AtomicBool>,
    pub(super) reset: Arc<AtomicU64>,
    pub(super) callback: CallbackSlot,
    pub(super) diag: DiagnosticsHandle,
}

pub(super) fn run(runtime: &Runtime, ready: &Sender<Result<(), String>>) -> WakeWordResult<()> {
    let mut detector = DecoderSession::new(
        SherpaDetector::new(&runtime.config)?,
        runtime.config.sample_rate,
    );
    let mut recovery = AudioRecovery::default();
    let mut initialized = false;
    while runtime.running.load(Ordering::Acquire) {
        detector.reset();
        let started = Instant::now();
        match capture(runtime, ready, &mut initialized, &mut detector) {
            Ok(()) => return Ok(()),
            Err(error) if runtime.running.load(Ordering::Acquire) => {
                let Some(delay) = recovery.next(
                    &error,
                    if initialized {
                        started.elapsed()
                    } else {
                        Duration::ZERO
                    },
                ) else {
                    return Err(WakeWordError::Audio(format!("{error}. Восстановление недоступно: выключите и включите пробуждение заново.")));
                };
                diag::set_running(&runtime.diag, false);
                *runtime.status.lock() = WakeWordStatus::Error;
                runtime.callback.notify(WakeWordEvent::Error {
                    message: format!(
                        "{error}. Переподключение микрофона {}/3",
                        recovery.attempt()
                    ),
                });
                *runtime.status.lock() = WakeWordStatus::Loading;
                runtime.callback.notify(WakeWordEvent::ModelLoading);
                if !wait_while_running(&runtime.running, delay) {
                    return Ok(());
                }
            }
            Err(_) => return Ok(()),
        }
    }
    Ok(())
}
fn capture(
    runtime: &Runtime,
    ready: &Sender<Result<(), String>>,
    initialized: &mut bool,
    detector: &mut DecoderSession<SherpaDetector>,
) -> WakeWordResult<()> {
    let (tx, rx) = bounded::<AudioPacket>(64);
    let overflow = Arc::new(AtomicBool::new(false));
    let capture_overflow = overflow.clone();
    let _subscription = runtime.hub.subscribe_packets(
        runtime.config.audio_device_id.as_deref(),
        runtime.config.sample_rate,
        move |packet| {
            if tx.try_send(packet).is_err() {
                capture_overflow.store(true, Ordering::Release);
            }
        },
    )?;
    let paused = runtime.paused.load(Ordering::Acquire);
    *runtime.status.lock() = if paused {
        WakeWordStatus::Paused
    } else {
        WakeWordStatus::Listening
    };
    diag::set_running(&runtime.diag, true);
    diag::set_paused(&runtime.diag, paused);
    runtime.callback.notify(if paused {
        WakeWordEvent::Paused
    } else {
        WakeWordEvent::Listening
    });
    if !*initialized {
        let _ = ready.send(Ok(()));
        *initialized = true;
    }
    processing_loop(runtime, detector, rx, overflow)
}
fn processing_loop(
    runtime: &Runtime,
    detector: &mut DecoderSession<SherpaDetector>,
    rx: Receiver<AudioPacket>,
    overflow: Arc<AtomicBool>,
) -> WakeWordResult<()> {
    let mut reset = runtime.reset.load(Ordering::Acquire);
    let mut expected: Option<AudioCursor> = None;
    let mut last_detection: Option<u64> = None;
    let rate = runtime.config.sample_rate as u64;
    let mut samples = Vec::new();
    let mut latest_packet = Instant::now();
    while runtime.running.load(Ordering::Acquire) {
        let packet = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(p) => p,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if let Some(message) = runtime.hub.diagnostics().last_error {
                    return Err(WakeWordError::Audio(message));
                }
                if latest_packet.elapsed() >= Duration::from_secs(2) {
                    return Err(WakeWordError::Audio(
                        "microphone stopped producing frames (disconnect or suspend)".into(),
                    ));
                }
                continue;
            }
            Err(_) => break,
        };
        if latest_packet.elapsed() >= Duration::from_secs(2) {
            return Err(WakeWordError::Audio(
                "microphone resumed after an interrupted sample stream".into(),
            ));
        }
        latest_packet = Instant::now();
        if overflow.swap(false, Ordering::AcqRel) {
            return Err(WakeWordError::Audio(
                "wake decoder fell behind the bounded audio queue".into(),
            ));
        }
        let current_reset = runtime.reset.load(Ordering::Acquire);
        if current_reset != reset || runtime.paused.load(Ordering::Acquire) {
            detector.reset();
            expected = None;
            reset = current_reset;
            if runtime.paused.load(Ordering::Acquire) {
                continue;
            }
            while rx.try_recv().is_ok() {}
            continue;
        }
        if expected.is_some_and(|e| e.epoch != packet.epoch || e.sample != packet.start_sample) {
            return Err(WakeWordError::Audio(
                "audio frames were lost before wake decoding".into(),
            ));
        }
        expected = Some(AudioCursor {
            epoch: packet.epoch,
            sample: packet.end_sample,
        });
        samples.clear();
        samples.extend(packet.samples.iter().map(|&s| s as f32 / 32768.0));
        diag::update_audio_level(&runtime.diag, &samples);
        if let Some(result) = detector.accept(&samples, packet.end_sample) {
            let detection = result.detection;
            let allow = last_detection
                .map(|last| {
                    packet.end_sample.saturating_sub(last)
                        >= runtime.config.cooldown_ms * rate / 1000
                })
                .unwrap_or(true);
            if allow {
                last_detection = Some(packet.end_sample);
                let cursor = AudioCursor {
                    epoch: packet.epoch,
                    sample: result.phrase_end_sample,
                };
                diag::record_result(&runtime.diag, &detection.phrase, &detection.json);
                runtime.paused.store(true, Ordering::Release);
                *runtime.status.lock() = WakeWordStatus::Paused;
                runtime.callback.notify(WakeWordEvent::Detected {
                    phrase: detection.phrase,
                    pre_roll: Vec::new(),
                    audio_cursor: Some(cursor),
                });
                continue;
            }
        }
    }
    Ok(())
}
