//! One opt-in scheduler. It checks the feed only; it never downloads or installs.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use super::UpdateService;
use crate::state::AppState;

pub(super) const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Scheduler {
    started: AtomicBool,
    changed: Notify,
}

pub(super) struct Schedule {
    last_attempt: Option<u64>,
}

impl Schedule {
    fn new(last_attempt: Option<u64>, now: u64) -> Self {
        // A timestamp from a clock that was moved forward must not suppress
        // background checks for years after correcting the system clock.
        Self {
            last_attempt: last_attempt.map(|last| last.min(now)),
        }
    }

    pub(super) fn delay(&self, now: u64) -> Duration {
        self.last_attempt.map_or(Duration::ZERO, |last| {
            Duration::from_secs(INTERVAL.as_secs().saturating_sub(now.saturating_sub(last)))
        })
    }

    pub(super) fn record_attempt(&mut self, now: u64) {
        self.last_attempt = Some(now);
    }
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn start(app: AppHandle) {
    if app
        .state::<UpdateService>()
        .automatic
        .started
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    tauri::async_runtime::spawn(run(app));
}

pub(super) fn changed(app: &AppHandle) {
    let service = app.state::<UpdateService>();
    if !app.state::<AppState>().settings().update_checks_enabled {
        service.cancel_automatic_check();
    }
    start(app.clone());
    service.automatic.changed.notify_one();
}

async fn run(app: AppHandle) {
    let last_attempt = crate::state::load_update_check_time().unwrap_or_else(|_| {
        tracing::warn!("could not read automatic update check timestamp");
        None
    });
    let now = unix_now();
    if last_attempt.is_some_and(|last| last > now)
        && crate::state::save_update_check_time(now).is_err()
    {
        tracing::warn!("could not rebase automatic update check timestamp");
    }
    let mut schedule = Schedule::new(last_attempt, now);
    loop {
        let service = app.state::<UpdateService>();
        if service.shutdown_started.load(Ordering::Acquire) {
            return;
        }
        if !app.state::<AppState>().settings().update_checks_enabled || service.channel.is_none() {
            service.automatic.changed.notified().await;
            continue;
        }
        let delay = schedule.delay(unix_now());
        if !delay.is_zero() {
            wait(&service.automatic.changed, delay).await;
            continue;
        }
        match service.check_automatically(&app, &mut schedule).await {
            Ok(true) => (),
            Ok(false) => wait(&service.automatic.changed, BUSY_RETRY).await,
            Err(error) => {
                // Busy does not consume the daily attempt. Network/storage
                // failures do, preventing repeated requests across restarts.
                if !matches!(error, crate::error::AppError::Busy(_)) {
                    tracing::warn!("automatic update check did not complete");
                }
                wait(&service.automatic.changed, BUSY_RETRY).await;
            }
        }
    }
}

async fn wait(changed: &Notify, delay: Duration) {
    tokio::select! {
        _ = changed.notified() => (),
        _ = tokio::time::sleep(delay) => (),
    }
}

#[cfg(test)]
mod tests;
