//! One explicit replacement attempt per captured selection, including unknown outcomes.
use super::*;
use crate::injection::target::AppendResult;

#[derive(Clone, Serialize)]
pub(crate) struct DeliverySnapshot {
    pub phase: &'static str,
    operation_id: Option<u64>,
    pub(super) result_id: Option<u64>,
    replace_allowed: bool,
    message: Option<String>,
}
struct Pending {
    id: u64,
    result: u64,
    copy: bool,
    cancel: OperationCancellation,
}
#[derive(Default)]
pub(super) struct Delivery {
    armed: Option<u64>,
    attempted: bool,
    pending: Option<Pending>,
    outcome: Option<&'static str>,
    message: Option<String>,
}
impl Delivery {
    pub(super) fn snapshot(&self, fresh: bool) -> DeliverySnapshot {
        DeliverySnapshot {
            phase: self.pending.as_ref().map_or_else(
                || {
                    if self.armed.is_some() {
                        "armed"
                    } else {
                        self.outcome.unwrap_or("idle")
                    }
                },
                |p| {
                    if p.cancel.is_cancelled() {
                        "cancelling"
                    } else if p.copy {
                        "copying"
                    } else {
                        "replacing"
                    }
                },
            ),
            operation_id: self.pending.as_ref().map(|p| p.id),
            result_id: self.pending.as_ref().map(|p| p.result).or(self.armed),
            replace_allowed: fresh && !self.attempted && self.pending.is_none(),
            message: self.message.clone(),
        }
    }
    fn arm(&mut self, result: u64) -> AppResult<()> {
        if self.attempted || self.pending.is_some() {
            return Err(blocked());
        }
        self.armed = Some(result);
        self.message = None;
        Ok(())
    }
    pub(super) fn invalidate_prepared(&mut self) -> AppResult<()> {
        if self.pending.is_some() {
            return Err(blocked());
        }
        self.armed = None;
        Ok(())
    }
    pub(super) fn cancel(&mut self) {
        self.armed = None;
        if let Some(p) = &self.pending {
            p.cancel.cancel();
        }
    }
    fn claim(&mut self, id: u64, result: u64, copy: bool) -> AppResult<OperationCancellation> {
        if self.pending.is_some() || (!copy && (self.attempted || self.armed != Some(result))) {
            return Err(blocked());
        }
        self.armed = None;
        if !copy {
            self.attempted = true;
        }
        let cancel = OperationCancellation::default();
        self.pending = Some(Pending {
            id,
            result,
            copy,
            cancel: cancel.clone(),
        });
        self.message = None;
        Ok(cancel)
    }
    fn finish(&mut self, id: u64, result: AppResult<AppendResult>, expected: usize) -> bool {
        let Some(p) = self.pending.as_ref().filter(|p| p.id == id) else {
            return false;
        };
        let copy = p.copy;
        let cancelled = p.cancel.is_cancelled();
        let phase = match &result {
            Ok(_) if copy => "copied",
            Err(_) if copy => "idle",
            Ok(r) if !cancelled && !r.paused && !r.uncertain && r.bytes == expected => "submitted",
            Ok(r) if r.bytes == 0 && !r.uncertain => "blocked",
            _ => "uncertain",
        };
        self.message = match &result {
            Err(e) => Some(e.user_error().params.detail.unwrap_or_else(|| if copy {
                "Не удалось скопировать результат. Текст сохранён; повторите копирование.".into()
            } else { "Действие не подтверждено. Результат доступен для копирования; повторная вставка заблокирована.".into() })),
            _ if matches!(phase, "blocked" | "uncertain") => Some("Действие не подтверждено. Результат доступен для копирования; повторная вставка заблокирована.".into()),
            _ => None,
        };
        self.pending = None;
        self.outcome = Some(phase);
        true
    }
}
fn blocked() -> AppError {
    AppError::Busy("Повторная замена этого выделения недоступна. Скопируйте результат или получите выделение заново.".into())
}
fn with_active<T>(
    app: &AppHandle,
    selection: u64,
    action: impl FnOnce(&mut Active) -> AppResult<T>,
) -> AppResult<T> {
    let runtime = app.state::<Runtime>();
    let mut slot = runtime.slot.lock();
    let a = slot
        .active
        .as_mut()
        .filter(|a| a.id == selection)
        .ok_or_else(|| {
            AppError::Cancelled("Это выделение уже закрыто. Получите его заново.".into())
        })?;
    action(a)
}
pub(crate) fn arm(app: &AppHandle, selection: u64, result: u64) -> AppResult<()> {
    with_active(app, selection, |a| {
        a.edit.output(result, true)?;
        a.delivery.arm(result)
    })?;
    notify(app);
    Ok(())
}
pub(crate) fn disarm(app: &AppHandle, selection: u64, result: u64) {
    let _ = with_active(app, selection, |a| {
        if a.delivery.armed == Some(result)
            || a.delivery
                .pending
                .as_ref()
                .is_some_and(|p| p.result == result)
        {
            a.delivery.cancel();
        }
        Ok(())
    });
    notify(app);
}
pub(crate) fn reject(app: &AppHandle, selection: u64, expected_result: u64, error: AppError) {
    let _ = with_active(app, selection, |a| {
        if a.delivery.armed == Some(expected_result) {
            a.delivery.armed = None;
            a.delivery.attempted = true;
            a.delivery.outcome = Some("blocked");
            a.delivery.message = Some(error.user_error().params.detail.unwrap_or_else(|| "Действие не подтверждено. Результат доступен для копирования; повторная вставка заблокирована.".into()));
        }
        Ok(())
    });
    notify(app);
}
pub(crate) fn submit(app: AppHandle, selection: u64, expected_result: u64) -> AppResult<()> {
    let lease = activity::lease()?;
    let configuration =
        crate::application::capture_configuration::try_reserve_configuration(|| {
            let pipeline = app.state::<Pipeline>();
            if pipeline.current_operation().is_some()
                || pipeline.has_active_service_operation()
                || crate::application::dictation::workflow::is_busy(&app)
            {
                return Err(AppError::Busy("Сначала завершите текущую диктовку.".into()));
            }
            Ok(())
        })?;
    let mode = app.state::<AppState>().settings().injection_mode;
    let runtime = app.state::<Runtime>();
    let operation = runtime.next.fetch_add(1, Ordering::Relaxed) + 1;
    let (original, text, cancel) = with_active(&app, selection, |a| {
        let result = a.delivery.armed.ok_or_else(blocked)?;
        if result != expected_result {
            return Err(blocked());
        }
        let text = a.edit.output(result, true)?;
        let original = match a.result.as_ref() {
            Some(Ok(value)) if value.belongs_to(selection) => value.clone(),
            _ => return Err(blocked()),
        };
        let cancel = a.delivery.claim(operation, result, false)?;
        Ok((original, text, cancel))
    })?;
    notify(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let _configuration = configuration;
        let expected = text.len();
        let result = if cancel.is_cancelled() {
            Err(blocked())
        } else {
            selection::replace(selection, original, &text, mode, cancel)
        };
        finish(&app, selection, operation, result, expected);
    });
    Ok(())
}
pub(crate) fn copy(app: AppHandle, selection: u64, result: u64) -> AppResult<()> {
    let lease = activity::lease()?;
    let runtime = app.state::<Runtime>();
    let operation = runtime.next.fetch_add(1, Ordering::Relaxed) + 1;
    let (text, cancel) = with_active(&app, selection, |a| {
        let text = a.edit.output(result, false)?;
        let cancel = a.delivery.claim(operation, result, true)?;
        Ok((text, cancel))
    })?;
    notify(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let expected = text.len();
        let result = if cancel.is_cancelled() {
            Err(blocked())
        } else {
            crate::injection::copy_text(&text).map(|()| AppendResult {
                bytes: expected,
                paused: false,
                uncertain: false,
            })
        };
        finish(&app, selection, operation, result, expected);
    });
    Ok(())
}
fn finish(
    app: &AppHandle,
    selection: u64,
    operation: u64,
    result: AppResult<AppendResult>,
    expected: usize,
) {
    let _ = with_active(app, selection, |a| {
        a.delivery.finish(operation, result, expected);
        Ok(())
    });
    notify(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_explicit_arm_admits_one_attempt_and_old_results_cannot_reuse_it() {
        let mut gate = Delivery::default();
        assert!(gate.claim(10, 7, false).is_err());
        gate.arm(7).unwrap();
        assert!(gate.claim(10, 8, false).is_err());
        gate.claim(10, 7, false).unwrap();
        assert!(gate.claim(11, 7, false).is_err());
        assert!(!gate.finish(
            9,
            Ok(AppendResult {
                bytes: 4,
                paused: false,
                uncertain: false
            }),
            4
        ));
        assert!(gate.finish(
            10,
            Ok(AppendResult {
                bytes: 4,
                paused: false,
                uncertain: false
            }),
            4
        ));
        assert_eq!(gate.snapshot(true).phase, "submitted");
        assert!(gate.arm(7).is_err());
        assert!(gate.arm(8).is_err());
    }
    #[test]
    fn focus_loss_partial_input_and_errors_allow_copy_but_never_an_input_retry() {
        for result in [
            Ok(AppendResult {
                bytes: 0,
                paused: true,
                uncertain: false,
            }),
            Ok(AppendResult {
                bytes: 2,
                paused: true,
                uncertain: false,
            }),
            Ok(AppendResult {
                bytes: 0,
                paused: false,
                uncertain: true,
            }),
            Err(AppError::Injection("PRIVATE OS ERROR".into())),
        ] {
            let mut gate = Delivery::default();
            gate.arm(1).unwrap();
            gate.claim(2, 1, false).unwrap();
            gate.finish(2, result, 8);
            assert!(!gate.snapshot(true).replace_allowed);
            assert!(gate.arm(1).is_err());
            assert!(!gate.snapshot(true).message.unwrap().contains("PRIVATE"));
            gate.claim(3, 1, true).unwrap();
            gate.finish(
                3,
                Ok(AppendResult {
                    bytes: 8,
                    paused: false,
                    uncertain: false,
                }),
                8,
            );
            assert_eq!(gate.snapshot(true).phase, "copied");
            assert!(gate.arm(1).is_err());
        }
    }
    #[test]
    fn cancellation_and_new_edit_invalidate_preparation_without_releasing_real_work() {
        let mut gate = Delivery::default();
        gate.arm(1).unwrap();
        gate.invalidate_prepared().unwrap();
        assert!(gate.claim(2, 1, false).is_err());
        gate.arm(3).unwrap();
        let cancel = gate.claim(4, 3, false).unwrap();
        gate.cancel();
        assert!(cancel.is_cancelled());
        assert!(gate.invalidate_prepared().is_err());
        assert_eq!(gate.snapshot(true).phase, "cancelling");
        gate.finish(
            4,
            Ok(AppendResult {
                bytes: 0,
                paused: true,
                uncertain: false,
            }),
            8,
        );
        assert!(gate.arm(3).is_err());
    }
}
