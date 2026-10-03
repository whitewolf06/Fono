use super::owner::run_owner;
use super::{ring::SampleRing, AudioCursor, AudioPacket};
use crate::error::{WakeWordError, WakeWordResult};
use crossbeam_channel::{bounded, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AudioConfig {
    pub(super) device_id: Option<String>,
    pub(super) sample_rate: u32,
}
pub(super) struct Dispatch {
    pub(super) subscribers: BTreeMap<u64, Sender<AudioPacket>>,
    pub(super) ring: SampleRing,
    pub(super) last_error: Option<String>,
}
pub(super) type SharedDispatch = Arc<Mutex<Dispatch>>;

#[derive(Debug, Clone, Serialize)]
pub struct AudioHubDiagnostics {
    pub cursor: AudioCursor,
    pub dropped_capture_packets: u64,
    pub dropped_subscriber_packets: u64,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct AudioHub {
    inner: Arc<HubInner>,
}
struct HubInner {
    commands: Sender<Command>,
    dispatch: SharedDispatch,
    next_id: AtomicU64,
    dropped: Arc<AtomicU64>,
    subscriber_dropped: Arc<AtomicU64>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
pub(super) enum Command {
    Ensure {
        config: AudioConfig,
        response: Sender<WakeWordResult<()>>,
    },
    StopIfUnused,
    Shutdown {
        response: Sender<()>,
    },
}
pub struct AudioSubscription {
    id: u64,
    inner: Arc<HubInner>,
    worker: Option<JoinHandle<()>>,
}

impl AudioHub {
    pub const MAX_SUBSCRIBERS: usize = 4;
    pub fn new() -> Self {
        let (commands, receiver) = bounded(8);
        let dispatch = Arc::new(Mutex::new(Dispatch {
            subscribers: BTreeMap::new(),
            ring: SampleRing::new(240_000),
            last_error: None,
        }));
        let dropped = Arc::new(AtomicU64::new(0));
        let subscriber_dropped = Arc::new(AtomicU64::new(0));
        let owner_dispatch = dispatch.clone();
        let owner_dropped = dropped.clone();
        let owner_subscriber_dropped = subscriber_dropped.clone();
        let thread = thread::spawn(move || {
            run_owner(
                receiver,
                owner_dispatch,
                owner_dropped,
                owner_subscriber_dropped,
            )
        });
        Self {
            inner: Arc::new(HubInner {
                commands,
                dispatch,
                next_id: AtomicU64::new(1),
                dropped,
                subscriber_dropped,
                thread: Mutex::new(Some(thread)),
            }),
        }
    }
    pub fn subscribe<F>(
        &self,
        device_id: Option<&str>,
        sample_rate: u32,
        callback: F,
    ) -> WakeWordResult<AudioSubscription>
    where
        F: Fn(&[i16]) + Send + Sync + 'static,
    {
        self.subscribe_packets(device_id, sample_rate, move |packet| {
            callback(&packet.samples)
        })
    }
    pub fn subscribe_packets<F>(
        &self,
        device_id: Option<&str>,
        sample_rate: u32,
        callback: F,
    ) -> WakeWordResult<AudioSubscription>
    where
        F: Fn(AudioPacket) + Send + Sync + 'static,
    {
        self.subscribe_impl(device_id, sample_rate, None, callback)
    }
    /// Register live delivery and replay the retained tail under one dispatch
    /// lock. Replay and subsequent packets use the same FIFO and sample clock.
    pub fn subscribe_packets_from<F>(
        &self,
        device_id: Option<&str>,
        sample_rate: u32,
        cursor: AudioCursor,
        callback: F,
    ) -> WakeWordResult<AudioSubscription>
    where
        F: Fn(AudioPacket) + Send + Sync + 'static,
    {
        self.subscribe_impl(device_id, sample_rate, Some(cursor), callback)
    }
    fn subscribe_impl<F>(
        &self,
        device_id: Option<&str>,
        sample_rate: u32,
        cursor: Option<AudioCursor>,
        callback: F,
    ) -> WakeWordResult<AudioSubscription>
    where
        F: Fn(AudioPacket) + Send + Sync + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = bounded::<AudioPacket>(2048);
        // Reserve before Ensure so a second device cannot replace a live input.
        {
            let mut dispatch = self.inner.dispatch.lock();
            if dispatch.subscribers.len() >= Self::MAX_SUBSCRIBERS {
                return Err(WakeWordError::Audio(
                    "audio subscriber limit reached".into(),
                ));
            }
            dispatch.subscribers.insert(id, tx.clone());
        }
        let (response, response_rx) = bounded(1);
        let config = AudioConfig {
            device_id: device_id.map(str::to_owned),
            sample_rate,
        };
        let result = self
            .inner
            .commands
            .send(Command::Ensure { config, response })
            .map_err(|_| WakeWordError::Audio("audio owner is unavailable".into()))
            .and_then(|_| {
                response_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| {
                        WakeWordError::Audio("audio input initialization timed out".into())
                    })?
            });
        if let Err(error) = result {
            self.inner.dispatch.lock().subscribers.remove(&id);
            let _ = self.inner.commands.try_send(Command::StopIfUnused);
            return Err(error);
        }
        if let Some(cursor) = cursor {
            let mut dispatch = self.inner.dispatch.lock();
            // Packets queued between reserve and replay are superseded by ring.
            while rx.try_recv().is_ok() {}
            match dispatch.ring.since(cursor) {
                Ok(packets) => {
                    for packet in packets {
                        if tx.try_send(packet).is_err() {
                            dispatch.subscribers.remove(&id);
                            return Err(WakeWordError::Audio(
                                "handoff queue capacity exceeded".into(),
                            ));
                        }
                    }
                }
                Err(message) => {
                    dispatch.subscribers.remove(&id);
                    let _ = self.inner.commands.try_send(Command::StopIfUnused);
                    return Err(WakeWordError::Audio(message));
                }
            }
        }
        drop(tx);
        let worker = thread::spawn(move || {
            while let Ok(packet) = rx.recv() {
                callback(packet);
            }
        });
        Ok(AudioSubscription {
            id,
            inner: self.inner.clone(),
            worker: Some(worker),
        })
    }
    pub fn cursor(&self) -> AudioCursor {
        self.inner.dispatch.lock().ring.cursor
    }
    pub fn snapshot_since(&self, cursor: AudioCursor) -> WakeWordResult<Vec<AudioPacket>> {
        self.inner
            .dispatch
            .lock()
            .ring
            .since(cursor)
            .map_err(WakeWordError::Audio)
    }
    pub fn diagnostics(&self) -> AudioHubDiagnostics {
        let dispatch = self.inner.dispatch.lock();
        AudioHubDiagnostics {
            cursor: dispatch.ring.cursor,
            dropped_capture_packets: self.inner.dropped.load(Ordering::Relaxed),
            dropped_subscriber_packets: self.inner.subscriber_dropped.load(Ordering::Relaxed),
            last_error: dispatch.last_error.clone(),
        }
    }
    pub fn shutdown(&self) {
        let Some(thread) = self.inner.thread.lock().take() else {
            return;
        };
        let (response, rx) = bounded(1);
        if self
            .inner
            .commands
            .send(Command::Shutdown { response })
            .is_ok()
        {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
        let _ = thread.join();
    }
}
impl Default for AudioHub {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for HubInner {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.get_mut().take() {
            let (response, rx) = bounded(1);
            if self.commands.send(Command::Shutdown { response }).is_ok() {
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
            let _ = thread.join();
        }
    }
}
impl Drop for AudioSubscription {
    fn drop(&mut self) {
        self.inner.dispatch.lock().subscribers.remove(&self.id);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = self.inner.commands.try_send(Command::StopIfUnused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_is_idempotent() {
        let hub = AudioHub::new();
        hub.shutdown();
        hub.shutdown();
    }
    #[test]
    fn clone_drop_does_not_shutdown_the_owner() {
        let hub = AudioHub::new();
        drop(hub.clone());
        assert!(hub.inner.thread.lock().is_some());
    }
}
