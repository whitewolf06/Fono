//! Serialize capture admission with asynchronous settings preparation.
//! The capture lock is synchronous; a configuration reservation keeps only a
//! busy flag across await points and can outlive its cancelled IPC caller.
#[cfg(test)]
mod tests;

use crate::error::{AppError, AppResult};
use parking_lot::{Mutex, MutexGuard};
use std::sync::Arc;

#[derive(Default)]
struct GateState {
    configuring: bool,
}

#[derive(Default)]
struct Gate(Mutex<GateState>);

static GATE: Gate = Gate(Mutex::new(GateState { configuring: false }));

pub(crate) struct CaptureAdmission<'a> {
    _guard: MutexGuard<'a, GateState>,
}

struct Reservation<'a> {
    gate: &'a Gate,
}

/// Clones protect actual background preparation after an IPC future is dropped.
#[derive(Clone)]
pub(crate) struct ConfigurationReservation<'a> {
    _reservation: Arc<Reservation<'a>>,
}

fn busy() -> AppError {
    AppError::Busy(
        "Настройки записи применяются. Повторите действие после завершения сохранения.".into(),
    )
}

impl Gate {
    fn begin_capture(&self) -> AppResult<CaptureAdmission<'_>> {
        let guard = self.0.lock();
        if guard.configuring {
            return Err(busy());
        }
        Ok(CaptureAdmission { _guard: guard })
    }

    fn reserve_configuration(
        &self,
        validate_active_operation: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<ConfigurationReservation<'_>> {
        let mut guard = self.0.lock();
        if guard.configuring {
            return Err(busy());
        }
        // A capture start holds this same lock until its operation and settings
        // snapshot are registered, so validation cannot race with that start.
        validate_active_operation()?;
        guard.configuring = true;
        Ok(ConfigurationReservation {
            _reservation: Arc::new(Reservation { gate: self }),
        })
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.gate.0.lock().configuring = false;
    }
}

/// Hold through reading settings and registering the synchronous capture start.
pub(crate) fn begin_capture() -> AppResult<CaptureAdmission<'static>> {
    GATE.begin_capture()
}

/// Atomically validate idle/capture policy and prevent new capture admissions.
pub(crate) fn reserve_configuration(
    validate_active_operation: impl FnOnce() -> AppResult<()>,
) -> AppResult<ConfigurationReservation<'static>> {
    GATE.reserve_configuration(validate_active_operation)
}
