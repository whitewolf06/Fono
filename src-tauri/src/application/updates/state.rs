use parking_lot::Mutex;
use serde::Serialize;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::watch;

use super::configuration::Channel;

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    NotConfigured,
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Error,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub phase: UpdatePhase,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_version: Option<String>,
    pub downloaded_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    pub message: String,
    pub checks_enabled: bool,
}

#[cfg(windows)]
pub(super) struct PendingUpdate {
    pub update: tauri_plugin_updater::Update,
    /// Constructed only by plugin download after signature + version checking.
    pub verified_bytes: Option<Arc<Vec<u8>>>,
}

pub(super) struct UpdateData {
    pub snapshot: UpdateSnapshot,
    #[cfg(windows)]
    pub pending: Option<PendingUpdate>,
    pub cancellation: Option<Arc<watch::Sender<u8>>>,
    #[cfg(windows)]
    pub automatic_check: bool,
}

pub struct UpdateService {
    pub(super) data: Mutex<UpdateData>,
    pub(super) serial: Arc<tokio::sync::Mutex<()>>,
    pub(super) settings_transaction: Arc<tokio::sync::Mutex<()>>,
    pub(super) channel: Option<Channel>,
    pub(super) shutdown_started: Arc<AtomicBool>,
    #[cfg(windows)]
    pub(super) automatic: super::automatic::Scheduler,
}

impl Default for UpdateService {
    fn default() -> Self {
        let channel = Channel::from_build();
        Self {
            data: Mutex::new(UpdateData {
                snapshot: UpdateSnapshot {
                    phase: if channel.is_some() {
                        UpdatePhase::Idle
                    } else {
                        UpdatePhase::NotConfigured
                    },
                    current_version: env!("CARGO_PKG_VERSION").into(),
                    next_version: None,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    message: if channel.is_some() {
                        "Проверка обновлений доступна."
                    } else {
                        "Канал подписанных обновлений ещё не настроен для этой сборки."
                    }
                    .into(),
                    checks_enabled: false,
                },
                #[cfg(windows)]
                pending: None,
                cancellation: None,
                #[cfg(windows)]
                automatic_check: false,
            }),
            serial: Arc::new(tokio::sync::Mutex::new(())),
            settings_transaction: Arc::new(tokio::sync::Mutex::new(())),
            channel,
            shutdown_started: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            automatic: super::automatic::Scheduler::default(),
        }
    }
}

impl UpdateService {
    pub fn snapshot(&self, checks_enabled: bool) -> UpdateSnapshot {
        let mut snapshot = self.data.lock().snapshot.clone();
        snapshot.checks_enabled = checks_enabled;
        snapshot
    }

    pub fn cancel(&self) -> bool {
        let data = self.data.lock();
        if !matches!(
            data.snapshot.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading
        ) {
            return false;
        }
        if let Some(cancellation) = &data.cancellation {
            cancellation.send_replace(1);
            true
        } else {
            false
        }
    }

    #[cfg(windows)]
    pub(super) fn cancel_automatic_check(&self) {
        let data = self.data.lock();
        if data.automatic_check && data.snapshot.phase == UpdatePhase::Checking {
            if let Some(cancellation) = &data.cancellation {
                cancellation.send_replace(1);
            }
        }
    }

    pub(super) fn transition(&self, phase: UpdatePhase, message: &str) {
        let mut data = self.data.lock();
        data.snapshot.phase = phase;
        data.snapshot.message = message.into();
    }
}

pub(super) async fn cancelled(receiver: &mut watch::Receiver<u8>) -> u8 {
    loop {
        let reason = *receiver.borrow_and_update();
        if reason != 0 {
            return reason;
        }
        if receiver.changed().await.is_err() {
            return 1;
        }
    }
}

/// Cleanup is also performed if a command future is dropped during shutdown.
pub(super) struct ActionGuard<'a>(pub &'a UpdateService);
impl Drop for ActionGuard<'_> {
    fn drop(&mut self) {
        let mut data = self.0.data.lock();
        data.cancellation = None;
        #[cfg(windows)]
        {
            data.automatic_check = false;
        }
        if matches!(
            data.snapshot.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading
        ) {
            data.snapshot.phase = if data.snapshot.next_version.is_some() {
                UpdatePhase::Available
            } else {
                UpdatePhase::Idle
            };
            data.snapshot.message = "Проверка или загрузка отменена.".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_drop_clears_transfer_and_does_not_leave_a_busy_snapshot() {
        let service = UpdateService::default();
        service.transition(UpdatePhase::Downloading, "progress");
        service.data.lock().snapshot.next_version = Some("1.2.3".into());
        let (sender, _) = watch::channel(0);
        service.data.lock().cancellation = Some(Arc::new(sender));
        {
            let _action = ActionGuard(&service);
        }
        assert_eq!(service.snapshot(false).phase, UpdatePhase::Available);
        assert!(!service.cancel());
    }

    #[test]
    fn cancellation_cannot_interrupt_installation() {
        let service = UpdateService::default();
        let (sender, _) = watch::channel(0);
        service.data.lock().cancellation = Some(Arc::new(sender));
        service.transition(UpdatePhase::Installing, "installing");
        assert!(!service.cancel());
    }

    #[cfg(windows)]
    #[test]
    fn opt_out_cancels_only_an_automatic_feed_check() {
        let service = UpdateService::default();
        let (sender, receiver) = watch::channel(0);
        {
            let mut data = service.data.lock();
            data.cancellation = Some(Arc::new(sender));
            data.snapshot.phase = UpdatePhase::Checking;
        }
        service.cancel_automatic_check();
        assert_eq!(*receiver.borrow(), 0, "manual check remains active");
        service.data.lock().automatic_check = true;
        service.cancel_automatic_check();
        assert_eq!(*receiver.borrow(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn opt_out_does_not_cancel_an_explicit_download_or_install() {
        let service = UpdateService::default();
        let (sender, receiver) = watch::channel(0);
        service.data.lock().cancellation = Some(Arc::new(sender));
        for phase in [UpdatePhase::Downloading, UpdatePhase::Installing] {
            service.transition(phase, "explicit operation");
            service.cancel_automatic_check();
            assert_eq!(*receiver.borrow(), 0);
        }
    }

    #[tokio::test]
    async fn cancellation_delivered_before_poll_is_not_lost() {
        let (sender, mut receiver) = watch::channel(0);
        sender.send_replace(1);
        assert_eq!(cancelled(&mut receiver).await, 1);
    }
}
