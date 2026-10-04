use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

#[test]
fn reserved_configuration_rejects_capture_and_concurrent_writer() {
    let gate = Gate::default();
    let reservation = gate.reserve_configuration(|| Ok(())).unwrap();
    assert!(gate.begin_capture().is_err());
    assert!(gate.reserve_configuration(|| Ok(())).is_err());
    drop(reservation);
    assert!(gate.begin_capture().is_ok());
    assert!(gate.reserve_configuration(|| Ok(())).is_ok());
}

#[test]
fn capture_registers_before_configuration_validates_or_reregisters_mode() {
    let gate = Gate::default();
    let active = AtomicBool::new(false);
    let registrations = AtomicUsize::new(0);
    let capture = gate.begin_capture().unwrap();
    let (attempting_tx, attempting_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            attempting_tx.send(()).unwrap();
            let result = gate.reserve_configuration(|| {
                if active.load(Ordering::Acquire) {
                    Err(AppError::Busy("Сначала завершите диктовку".into()))
                } else {
                    Ok(())
                }
            });
            if result.is_ok() {
                registrations.fetch_add(1, Ordering::Release);
            }
            finished_tx.send(result.is_err()).unwrap();
        });
        attempting_rx.recv().unwrap();
        assert!(finished_rx.try_recv().is_err());
        // Mirrors start_configured: publish operation before dropping admission.
        active.store(true, Ordering::Release);
        drop(capture);
        assert!(finished_rx.recv().unwrap());
    });
    assert_eq!(registrations.load(Ordering::Acquire), 0);
    active.store(false, Ordering::Release);
    assert!(gate.reserve_configuration(|| Ok(())).is_ok());
}

#[test]
fn failed_active_validation_never_reserves_or_changes_shortcuts() {
    let gate = Gate::default();
    let registrations = AtomicUsize::new(0);
    let result = gate.reserve_configuration(|| Err(AppError::Busy("Запись активна".into())));
    if result.is_ok() {
        registrations.fetch_add(1, Ordering::Relaxed);
    }
    assert!(result.is_err());
    assert_eq!(registrations.load(Ordering::Relaxed), 0);
    assert!(gate.begin_capture().is_ok());
}

#[test]
fn preparation_or_persistence_failure_releases_reservation() {
    fn prepare_and_fail(gate: &Gate) -> AppResult<()> {
        let _reservation = gate.reserve_configuration(|| Ok(()))?;
        Err(AppError::Config("Не удалось сохранить настройки".into()))
    }
    let gate = Gate::default();
    assert!(prepare_and_fail(&gate).is_err());
    assert!(gate.begin_capture().is_ok());
}

#[test]
fn dropped_ipc_keeps_capture_blocked_until_background_preparation_finishes() {
    let gate = Gate::default();
    let reservation = gate.reserve_configuration(|| Ok(())).unwrap();
    let background = reservation.clone();
    drop(reservation);
    assert!(gate.begin_capture().is_err());
    assert!(gate.reserve_configuration(|| Ok(())).is_err());
    drop(background);
    assert!(gate.begin_capture().is_ok());
}
