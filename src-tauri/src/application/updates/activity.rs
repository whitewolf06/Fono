//! Admission fence shared by synchronous starts and updater installation.
//! Always acquire it before pipeline, queue or download registry locks.
use parking_lot::{Mutex, MutexGuard};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::error::{AppError, AppResult};

#[derive(Default)]
struct GateState {
    reserved: bool,
    leases: usize,
}

#[derive(Default)]
struct Gate(Mutex<GateState>);

static GATE: Gate = Gate(Mutex::new(GateState {
    reserved: false,
    leases: 0,
}));

pub struct StartGuard<'a> {
    _guard: MutexGuard<'a, GateState>,
}

pub struct ActivityLease<'a> {
    gate: &'a Gate,
}

pub(super) struct InstallReservation<'a> {
    gate: &'a Gate,
    shutdown_started: Option<Arc<AtomicBool>>,
}

fn busy() -> AppError {
    AppError::Busy(
        "Приложение готовится установить обновление. Повторите действие после перезапуска.".into(),
    )
}

impl Gate {
    fn while_idle<T>(
        &self,
        check_idle: impl FnOnce() -> bool,
        action: impl FnOnce() -> T,
    ) -> Option<(T, ActivityLease<'_>)> {
        // GPU release already owns a nonblocking STT load gate. Do not wait
        // for a synchronous start that could itself be waiting for that gate.
        let mut state = self.0.try_lock()?;
        if state.reserved || state.leases > 0 || !check_idle() {
            return None;
        }
        // Only detachment belongs inside this fence. The returned lease holds
        // installer exclusion while native GPU/worker cleanup runs outside it.
        let result = action();
        state.leases += 1;
        Some((result, ActivityLease { gate: self }))
    }

    fn begin(&self) -> AppResult<StartGuard<'_>> {
        let guard = self.0.lock();
        if guard.reserved {
            return Err(busy());
        }
        Ok(StartGuard { _guard: guard })
    }

    fn lease(&self) -> AppResult<ActivityLease<'_>> {
        let mut state = self.0.lock();
        if state.reserved {
            return Err(busy());
        }
        state.leases += 1;
        Ok(ActivityLease { gate: self })
    }

    fn reserve(
        &self,
        check_idle: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<InstallReservation<'_>> {
        let mut state = self.0.lock();
        if state.reserved || state.leases > 0 {
            return Err(AppError::Busy(
                "Сначала завершите подготовку моделей и текущие операции.".into(),
            ));
        }
        check_idle()?;
        state.reserved = true;
        Ok(InstallReservation {
            gate: self,
            shutdown_started: None,
        })
    }
}

impl Drop for ActivityLease<'_> {
    fn drop(&mut self) {
        self.gate.0.lock().leases -= 1;
    }
}
impl Clone for ActivityLease<'_> {
    fn clone(&self) -> Self {
        self.gate.0.lock().leases += 1;
        Self { gate: self.gate }
    }
}

impl Drop for InstallReservation<'_> {
    fn drop(&mut self) {
        if !self
            .shutdown_started
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            self.gate.0.lock().reserved = false;
        }
    }
}

impl InstallReservation<'_> {
    pub(super) fn protect_shutdown(&mut self, flag: Arc<AtomicBool>) {
        self.shutdown_started = Some(flag);
    }
}

/// Hold until the synchronous start is registered in its owning state.
pub fn begin() -> AppResult<StartGuard<'static>> {
    GATE.begin()
}

/// Hold for an asynchronous preparation whose activity has no other registry.
pub fn lease() -> AppResult<ActivityLease<'static>> {
    GATE.lease()
}

pub(crate) fn while_idle<T>(
    check_idle: impl FnOnce() -> bool,
    action: impl FnOnce() -> T,
) -> Option<(T, ActivityLease<'static>)> {
    GATE.while_idle(check_idle, action)
}

pub(crate) fn is_busy() -> bool {
    let state = GATE.0.lock();
    state.reserved || state.leases > 0
}

pub(super) fn reserve(
    check_idle: impl FnOnce() -> AppResult<()>,
) -> AppResult<InstallReservation<'static>> {
    GATE.reserve(check_idle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn gpu_detachment_waits_for_operations_and_protects_cleanup() {
        let gate = Gate::default();
        let preparation = gate.lease().unwrap();
        assert!(gate
            .while_idle(|| true, || panic!("busy action ran"))
            .is_none());
        drop(preparation);
        assert!(gate
            .while_idle(|| false, || panic!("active action ran"))
            .is_none());
        let (_, cleanup) = gate.while_idle(|| true, || ()).unwrap();
        assert!(gate.reserve(|| Ok(())).is_err());
        // User work may start after detachment. The STT load gate makes it
        // wait for native destruction, while the installer remains excluded.
        assert!(gate.begin().is_ok());
        drop(cleanup);
        assert!(gate.reserve(|| Ok(())).is_ok());
    }

    #[test]
    fn installing_update_prevents_gpu_detachment() {
        let gate = Gate::default();
        let _install = gate.reserve(|| Ok(())).unwrap();
        assert!(gate
            .while_idle(|| true, || panic!("installing action ran"))
            .is_none());
    }

    #[test]
    fn synchronous_start_never_waits_on_gpu_release_admission() {
        let gate = Gate::default();
        let _start = gate.begin().unwrap();
        assert!(gate
            .while_idle(|| true, || panic!("start action ran"))
            .is_none());
    }

    #[test]
    fn reservation_rejects_starts_and_releases_after_install_failure() {
        let gate = Gate::default();
        let reservation = gate.reserve(|| Ok(())).unwrap();
        assert!(gate.begin().is_err());
        assert!(gate.lease().is_err());
        drop(reservation);
        assert!(gate.begin().is_ok());
    }

    #[test]
    fn preparation_and_active_snapshots_block_install_without_reserving() {
        let gate = Gate::default();
        let lease = gate.lease().unwrap();
        assert!(gate.reserve(|| Ok(())).is_err());
        drop(lease);
        assert!(gate
            .reserve(|| Err(AppError::Busy("dictation".into())))
            .is_err());
        assert!(gate.begin().is_ok());
    }

    #[test]
    fn dropped_ipc_does_not_release_a_blocking_install_or_preparation() {
        let gate = Gate::default();
        let lease = gate.lease().unwrap();
        let blocking_lease = lease.clone();
        drop(lease);
        assert!(gate.reserve(|| Ok(())).is_err());
        drop(blocking_lease);
        let mut reservation = gate.reserve(|| Ok(())).unwrap();
        reservation.protect_shutdown(Arc::new(AtomicBool::new(true)));
        drop(reservation);
        assert!(gate.begin().is_err());
    }

    #[test]
    fn moved_reservation_outlives_a_dropped_blocking_task_handle() {
        let gate = Gate::default();
        let reservation = gate.reserve(|| Ok(())).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let _reservation = reservation;
                started_tx.send(()).unwrap();
                finish_rx.recv().unwrap();
            });
            started_rx.recv().unwrap();
            drop(worker);
            assert!(gate.begin().is_err());
            assert!(gate.lease().is_err());
            finish_tx.send(()).unwrap();
        });
        assert!(gate.begin().is_ok());
    }

    #[test]
    fn panic_after_shutdown_keeps_the_admission_fence_sealed() {
        let gate = Gate::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut reservation = gate.reserve(|| Ok(())).unwrap();
        reservation.protect_shutdown(Arc::clone(&shutdown));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _reservation = reservation;
            shutdown.store(true, Ordering::Release);
            panic!("simulated installer callback panic");
        }));
        assert!(panic.is_err());
        assert!(gate.begin().is_err());
        assert!(gate.lease().is_err());
    }

    #[test]
    fn start_and_install_race_has_only_one_admitted_owner() {
        for _ in 0..50 {
            let gate = Arc::new(Gate::default());
            let activity = Arc::new(AtomicBool::new(false));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            std::thread::scope(|scope| {
                let g = Arc::clone(&gate);
                let a = Arc::clone(&activity);
                let b = Arc::clone(&barrier);
                let start = scope.spawn(move || {
                    b.wait();
                    if let Ok(_guard) = g.begin() {
                        a.store(true, Ordering::Release);
                        true
                    } else {
                        false
                    }
                });
                barrier.wait();
                let reservation = gate.reserve(|| {
                    if activity.load(Ordering::Acquire) {
                        Err(AppError::Busy("active".into()))
                    } else {
                        Ok(())
                    }
                });
                let started = start.join().unwrap();
                assert_ne!(started, reservation.is_ok());
            });
        }
    }
}
