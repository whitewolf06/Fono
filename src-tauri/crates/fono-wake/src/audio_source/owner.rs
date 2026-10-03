use super::{
    hub::{AudioConfig, Command, SharedDispatch},
    AudioStream,
};
use crate::error::{WakeWordError, WakeWordResult};
use crossbeam_channel::Receiver;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
pub(super) fn run_owner(
    rx: Receiver<Command>,
    dispatch: SharedDispatch,
    dropped: Arc<AtomicU64>,
    subscriber_dropped: Arc<AtomicU64>,
) {
    let mut active: Option<(AudioConfig, AudioStream)> = None;
    let mut epoch = 0;
    while let Ok(command) = rx.recv() {
        match command {
            Command::Ensure { config, response } => {
                let existing_error = dispatch.lock().last_error.clone();
                let result = if active.as_ref().is_some_and(|(c, _)| c == &config)
                    && existing_error.is_none()
                {
                    Ok(())
                } else if active.is_some() && dispatch.lock().subscribers.len() > 1 {
                    Err(WakeWordError::Audio(existing_error.unwrap_or_else(|| {
                        "consumers requested different audio inputs".into()
                    })))
                } else {
                    active.take();
                    epoch += 1;
                    {
                        let mut d = dispatch.lock();
                        d.ring.reset(epoch, config.sample_rate);
                        d.last_error = None;
                    }
                    start_stream(
                        &config,
                        dispatch.clone(),
                        dropped.clone(),
                        subscriber_dropped.clone(),
                    )
                    .map(|stream| active = Some((config, stream)))
                };
                let _ = response.send(result);
            }
            Command::StopIfUnused => {
                if dispatch.lock().subscribers.is_empty() {
                    active.take();
                }
            }
            Command::Shutdown { response } => {
                active.take();
                dispatch.lock().subscribers.clear();
                let _ = response.send(());
                break;
            }
        }
    }
}
fn start_stream(
    config: &AudioConfig,
    dispatch: SharedDispatch,
    dropped: Arc<AtomicU64>,
    subscriber_dropped: Arc<AtomicU64>,
) -> WakeWordResult<AudioStream> {
    let rate = config.sample_rate;
    let errors = dispatch.clone();
    AudioStream::start_with_errors(
        config.device_id.as_deref(),
        rate,
        move |samples| {
            let mut d = dispatch.lock();
            let packet = d.ring.push(samples, rate);
            let mut overflow = false;
            for subscriber in d.subscribers.values() {
                if subscriber.try_send(packet.clone()).is_err() {
                    subscriber_dropped.fetch_add(1, Ordering::Relaxed);
                    overflow = true;
                }
            }
            if overflow {
                d.last_error = Some("audio consumer overflow: frames were lost".into());
            }
        },
        move |message| {
            errors.lock().last_error = Some(message);
        },
        dropped,
    )
}
