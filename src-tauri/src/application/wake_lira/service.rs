use super::{
    worker::{WarmWorker, FRAME},
    Control, Phase, Runtime, Snapshot,
};
use crate::{
    error::{AppError, AppResult},
    pipeline::Pipeline,
    state::AppState,
};
use fono_audio::{AudioCursor, AudioHub, AudioPacket};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

const PRE_ROLL: usize = 64_000 + 10_240;
// A valid verifier reply may take up to the existing 3-second audio-age budget.
// 128 typical 10ms packets overflowed before that budget, discarding a confirmed
// phrase. Retain a bounded queue; the unchanged sample-age check rejects stale results.
const AUDIO_QUEUE_PACKETS: usize = 512;
fn interrupted_audio(reason: &'static str) -> AppError {
    tracing::warn!(
        event = "lira_audio_interrupted",
        reason,
        "live wake audio interrupted"
    );
    AppError::Audio(
        "Поток микрофона прервался или детектор не успевает за звуком. Включите пробуждение снова."
            .into(),
    )
}
fn phase(app: &AppHandle, status: &parking_lot::Mutex<Snapshot>, value: Phase) {
    let mut view = status.lock();
    if view.phase != Phase::Stopping && view.phase != value {
        view.phase = value;
        drop(view);
        tracing::info!(event = "lira_phase", phase = ?value, "live wake phase changed");
        crate::tray::request_refresh(app);
    }
}

/// Sample coordinates are retained through packet splitting and the capture handoff.
#[derive(Default)]
struct Frames {
    pending: VecDeque<i16>,
    pre_roll: VecDeque<i16>,
    cursor: Option<AudioCursor>,
}
impl Frames {
    fn clear(&mut self) {
        self.pending.clear();
        self.pre_roll.clear();
        self.cursor = None;
    }
    fn append(&mut self, packet: &AudioPacket) -> AppResult<()> {
        if packet.sample_rate != 16_000
            || packet.end_sample.saturating_sub(packet.start_sample) != packet.samples.len() as u64
        {
            return Err(interrupted_audio("packet_format"));
        }
        if let Some(cursor) = self.cursor {
            if cursor.epoch != packet.epoch
                || cursor.sample + self.pending.len() as u64 != packet.start_sample
            {
                return Err(interrupted_audio("packet_clock"));
            }
        } else {
            self.cursor = Some(AudioCursor {
                epoch: packet.epoch,
                sample: packet.start_sample,
            });
        }
        self.pending.extend(packet.samples.iter().copied());
        Ok(())
    }
    fn next(&mut self) -> Option<(Vec<i16>, AudioCursor)> {
        if self.pending.len() < FRAME {
            return None;
        }
        let pcm: Vec<_> = self.pending.drain(..FRAME).collect();
        self.pre_roll.extend(pcm.iter().copied());
        if self.pre_roll.len() > PRE_ROLL {
            self.pre_roll.drain(..self.pre_roll.len() - PRE_ROLL);
        }
        let cursor = self.cursor.as_mut()?;
        cursor.sample += FRAME as u64;
        Some((pcm, *cursor))
    }
}

pub(super) fn run(
    app: &AppHandle,
    id: u64,
    control: &Arc<Control>,
    status: &Arc<parking_lot::Mutex<Snapshot>>,
    device: Option<String>,
    warm: &mut Option<WarmWorker>,
) -> AppResult<()> {
    if warm.is_none() {
        *warm = Some(WarmWorker::start(control)?);
    }
    let worker = warm.as_mut().expect("initialized warm worker");
    let mut vad = super::super::speech_gate::SpeechGate::new(app)?;
    let hub = app.state::<AudioHub>();
    let (send, receive) = crossbeam_channel::bounded(AUDIO_QUEUE_PACKETS);
    let overflow = Arc::new(AtomicBool::new(false));
    let failed_send = Arc::clone(&overflow);
    let subscription = {
        let _configuration = super::super::capture_configuration::begin_capture()?;
        if control.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if app.state::<AppState>().settings().audio_device_id != device {
            return Err(interrupted_audio("device_changed"));
        }
        hub.subscribe_packets(device.as_deref(), 16_000, move |packet| {
            if send.try_send(packet).is_err() {
                failed_send.store(true, Ordering::Release);
            }
        })?
    };
    let mut frames = Frames::default();
    let mut suspended = true;
    let mut revision = control.revision.load(Ordering::Acquire);
    let mut last_audio = Instant::now();
    let started = Instant::now();
    let mut recovery = super::discontinuity::RecoveryBudget::default();
    let mut capture_losses = 0;
    loop {
        if control.cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if hub.diagnostics().last_error.is_some() {
            return Err(interrupted_audio("capture_failed"));
        }
        let paused = control.paused.load(Ordering::Acquire)
            || app.state::<AppState>().is_dictation_paused()
            || app.state::<Pipeline>().current_operation().is_some();
        let current = control.revision.load(Ordering::Acquire);
        let losses = subscription.dropped_capture_packets();
        if overflow.swap(false, Ordering::AcqRel) || losses != capture_losses {
            capture_losses = losses;
            if !paused && !recovery.allow(started.elapsed()) {
                return Err(interrupted_audio("repeated_packet_loss"));
            }
            tracing::warn!(
                event = "lira_audio_resync",
                reason = "packet_loss",
                paused,
                "discarding interrupted wake context; keeping warm models"
            );
            suspended = true;
            frames.clear();
            while receive.try_recv().is_ok() {}
            if !paused {
                phase(app, status, Phase::Loading);
            }
        }
        if paused {
            suspended = true;
            frames.clear();
            phase(app, status, Phase::Paused);
        } else if suspended || current != revision {
            // A late result or audio from a manual recording can never wake the next session.
            worker.reset(control)?;
            vad.reset();
            frames.clear();
            while receive.try_recv().is_ok() {}
            revision = current;
            suspended = false;
            last_audio = Instant::now();
            phase(app, status, Phase::Listening);
        }
        let packet = match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(packet) => packet,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if last_audio.elapsed() >= Duration::from_secs(3) {
                    return Err(interrupted_audio("audio_timeout"));
                }
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(interrupted_audio("audio_disconnected"))
            }
        };
        last_audio = Instant::now();
        if paused {
            continue;
        }
        if let Err(error) = frames.append(&packet) {
            if !recovery.allow(started.elapsed()) {
                return Err(error);
            }
            tracing::warn!(
                event = "lira_audio_resync",
                reason = "packet_clock",
                "discarding discontinuous wake context; keeping warm models"
            );
            suspended = true;
            phase(app, status, Phase::Loading);
            continue;
        }
        while let Some((pcm, cursor)) = frames.next() {
            let decision = vad.accept(&pcm);
            let recent_speech = decision.has_speech
                && decision.processed.saturating_sub(decision.last_speech) <= 64_000;
            let requested_at = Instant::now();
            let detected = worker.feed(&pcm, recent_speech, control)?;
            let now = hub.diagnostics();
            if now.last_error.is_some() || now.cursor.epoch != cursor.epoch {
                return Err(interrupted_audio("capture_failed_or_replaced"));
            }
            if control.revision.load(Ordering::Acquire) != revision
                || control.paused.load(Ordering::Acquire)
                || app.state::<AppState>().is_dictation_paused()
                || app.state::<Pipeline>().current_operation().is_some()
            {
                suspended = true;
                break;
            }
            if overflow.load(Ordering::Acquire)
                || subscription.dropped_capture_packets() != capture_losses
                || now.cursor.sample.saturating_sub(cursor.sample) > 48_000
            {
                if !recovery.allow(started.elapsed()) {
                    return Err(interrupted_audio("repeated_stale_audio"));
                }
                tracing::warn!(
                    event = "lira_audio_resync",
                    reason = "stale_audio",
                    lag_ms = now.cursor.sample.saturating_sub(cursor.sample) / 16,
                    "discarding late wake result; keeping warm models"
                );
                // Invalidate a possible detection before handing anything to dictation.
                suspended = true;
                frames.clear();
                while receive.try_recv().is_ok() {}
                overflow.store(false, Ordering::Release);
                capture_losses = subscription.dropped_capture_packets();
                phase(app, status, Phase::Loading);
                break;
            }
            if detected {
                let triggered_at = Instant::now();
                tracing::info!(
                    event = "lira_confirmed",
                    listener_session = id,
                    worker_roundtrip_ms = requested_at.elapsed().as_millis() as u64,
                    audio_queue_ms = now.cursor.sample.saturating_sub(cursor.sample) / 16,
                    "live wake phrase confirmed"
                );
                control.set_paused(true);
                let pre_roll = frames.pre_roll.iter().copied().collect();
                let (handle, owner) = (app.clone(), Arc::clone(control));
                tauri::async_runtime::spawn(async move {
                    let result = super::super::wake_dictation::run_lira(
                        handle.clone(),
                        owner,
                        pre_roll,
                        cursor,
                        triggered_at,
                    )
                    .await;
                    if handle.state::<Runtime>().is_current(id) {
                        if let Err(error) = result {
                            if !matches!(error, AppError::Cancelled(_) | AppError::Busy(_)) {
                                crate::events::emit_error(
                                    &handle,
                                    crate::events::ErrorCodeV1::Wake,
                                    error.user_message(),
                                    None,
                                );
                            }
                        }
                        super::super::dictation::resume_wake_if_idle(&handle);
                    }
                });
                suspended = true;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(start: u64, count: usize) -> AudioPacket {
        AudioPacket {
            samples: vec![7; count].into(),
            start_sample: start,
            end_sample: start + count as u64,
            epoch: 1,
            sample_rate: 16_000,
        }
    }
    #[test]
    fn verifier_pause_within_the_freshness_budget_does_not_overflow_input() {
        let (send, receive) = crossbeam_channel::bounded(AUDIO_QUEUE_PACKETS);
        // The capture thread continues while the synchronous verifier is busy.
        for index in 0..300 {
            send.try_send(packet(index * 160, 160)).unwrap();
        }
        let mut frames = Frames::default();
        let mut samples = 0;
        while let Ok(packet) = receive.try_recv() {
            frames.append(&packet).unwrap();
            while let Some((pcm, _)) = frames.next() {
                samples += pcm.len();
            }
        }
        assert_eq!(samples + frames.pending.len(), 48_000);
        assert_eq!(
            frames.cursor.unwrap().sample + frames.pending.len() as u64,
            48_000
        );
    }
    #[test]
    fn packet_splitting_preserves_handoff_cursor_and_bounded_preroll() {
        let mut frames = Frames::default();
        frames.append(&packet(100, 500)).unwrap();
        assert!(frames.next().is_none());
        frames.append(&packet(600, 1000)).unwrap();
        let (pcm, cursor) = frames.next().unwrap();
        assert_eq!(pcm.len(), FRAME);
        assert_eq!(cursor.sample, 1380);
        frames.append(&packet(1600, 100_000)).unwrap();
        while frames.next().is_some() {}
        assert_eq!(frames.pre_roll.len(), PRE_ROLL);
        assert!(frames.append(&packet(101_601, 1280)).is_err());
        frames.clear();
        frames.append(&packet(200_000, FRAME)).unwrap();
        assert_eq!(frames.next().unwrap().1.sample, 201_280);
        assert_eq!(frames.pre_roll.len(), FRAME);
    }
}
