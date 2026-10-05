use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};

use fono_setup::{Bootstrapper, Phase, Progress};

use crate::install::{self, Outcome};

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const INSTALLING: u8 = 2;

pub enum Event {
    Progress(Progress),
    Installing(String),
    Finished(Result<Outcome, String>),
}

pub struct Download {
    cancel: Arc<AtomicBool>,
    stage: Arc<AtomicU8>,
    pub events: Receiver<Event>,
}

impl Download {
    /// The winning transition owns the operation: cancellation or installation.
    /// A late Cancel cannot close the downloader while NSIS still uses its file.
    pub fn request_cancel(&self) -> bool {
        if self
            .stage
            .compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || self.stage.load(Ordering::Acquire) == CANCELLED
        {
            self.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
}

pub fn start() -> Download {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let stage = Arc::new(AtomicU8::new(ACTIVE));
    let worker_stage = Arc::clone(&stage);
    let (sender, events) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let config = fono_setup::config::embedded_config();
            let cache =
                fono_setup::config::default_cache_dir().map_err(|error| error.to_string())?;
            let bootstrapper =
                Bootstrapper::new(config, cache).map_err(|error| error.to_string())?;
            let last_update = Cell::new(None::<Instant>);
            let last_phase = Cell::new(Phase::Resolving);
            let installer = bootstrapper
                .run(Arc::clone(&worker_cancel), |progress| {
                    let now = Instant::now();
                    if progress.phase != last_phase.get()
                        || last_update.get().is_none_or(|last| {
                            now.duration_since(last) >= Duration::from_millis(100)
                        })
                        || progress.total_bytes == Some(progress.downloaded_bytes)
                    {
                        last_update.set(Some(now));
                        last_phase.set(progress.phase);
                        let _ = sender.send(Event::Progress(progress));
                    }
                })
                .map_err(|error| error.to_string())?;
            // In particular, an X/Cancel while signature verification is running
            // must not open UAC or the installer after verification finishes.
            if !claim_installation(&worker_stage) {
                return Ok(Outcome::Cancelled);
            }
            let _ = sender.send(Event::Installing(installer.version.clone()));
            install::launch_verified_package(&installer.path, &installer.version)
        })();
        let result = if worker_cancel.load(Ordering::Acquire) {
            Ok(Outcome::Cancelled)
        } else {
            result
        };
        let _ = sender.send(Event::Finished(result));
    });
    Download {
        cancel,
        stage,
        events,
    }
}

fn claim_installation(stage: &AtomicU8) -> bool {
    stage
        .compare_exchange(ACTIVE, INSTALLING, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation() -> Download {
        let (_sender, events) = mpsc::channel();
        Download {
            cancel: Arc::new(AtomicBool::new(false)),
            stage: Arc::new(AtomicU8::new(ACTIVE)),
            events,
        }
    }

    #[test]
    fn cancellation_before_installation_prevents_the_launch() {
        let operation = operation();
        assert!(operation.request_cancel());
        assert!(operation.cancel.load(Ordering::Acquire));
        assert!(!claim_installation(&operation.stage));
    }

    #[test]
    fn late_cancellation_does_not_release_an_open_installer() {
        let operation = operation();
        assert!(claim_installation(&operation.stage));
        assert!(!operation.request_cancel());
        assert!(!operation.cancel.load(Ordering::Acquire));
        assert!(!claim_installation(&operation.stage));
    }
}
