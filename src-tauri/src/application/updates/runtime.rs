use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use super::{
    activity,
    state::{ActionGuard, PendingUpdate},
    UpdatePhase, UpdateService,
};
use crate::error::{AppError, AppResult};
mod check;
mod transfer;

impl UpdateService {
    pub(super) async fn install(&self, app: &AppHandle) -> AppResult<()> {
        let serial = Arc::clone(&self.serial).try_lock_owned().map_err(|_| {
            AppError::Busy("Проверка или установка обновления уже выполняется.".into())
        })?;
        if self.channel.is_none() {
            return Err(AppError::Config(
                "Канал подписанных обновлений ещё не настроен.".into(),
            ));
        }
        let (mut update, cached) = {
            let data = self.data.lock();
            let pending = require_candidate(&data)?;
            (pending.update.clone(), pending.verified_bytes.clone())
        };
        ensure_idle(app)?;
        let (sender, mut receiver) = watch::channel(0u8);
        let cancellation = Arc::new(sender);
        self.data.lock().cancellation = Some(Arc::clone(&cancellation));
        let _action = ActionGuard(self);
        let bytes = match cached {
            Some(bytes) => bytes,
            None => {
                update.timeout = Some(transfer::DOWNLOAD_TIMEOUT);
                let Some(bytes) =
                    transfer::download(self, &update, &cancellation, &mut receiver).await?
                else {
                    return Ok(());
                };
                let bytes = Arc::new(bytes);
                if let Some(pending) = self.data.lock().pending.as_mut() {
                    pending.verified_bytes = Some(Arc::clone(&bytes));
                }
                bytes
            }
        };
        let mut reservation = match activity::reserve(|| ensure_idle(app)) {
            Ok(reservation) => reservation,
            Err(error) => {
                self.transition(
                    UpdatePhase::Available,
                    "Пакет проверен. Завершите текущие операции и повторите установку.",
                );
                return Err(error);
            }
        };
        reservation.protect_shutdown(Arc::clone(&self.shutdown_started));
        {
            // Installation is sealed under the same lock as cancel(). A
            // cancellation accepted before this point cannot launch anything.
            let mut data = self.data.lock();
            if *receiver.borrow() != 0 {
                data.snapshot.phase = UpdatePhase::Available;
                data.snapshot.message = "Установка отменена до запуска установщика.".into();
                return Ok(());
            }
            data.snapshot.phase = UpdatePhase::Installing;
            data.snapshot.message =
                "Запускаем проверенный установщик. Fono завершит работу.".into();
            data.cancellation = None;
        }
        // The IPC future only owns the cancellable transfer. The blocking task
        // owns all installation exclusivity and finalizes its snapshot itself.
        drop(_action);
        let install_app = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _serial = serial;
            let _reservation = reservation;
            let service = install_app.state::<UpdateService>();
            finish_install(&service, || {
                update.install(bytes.as_slice()).map_err(|_| ())
            })
        })
        .await;
        match result {
            Ok(result) => result,
            Err(_) => Err(AppError::Config(
                "Не удалось завершить установку. Перезапустите Fono.".into(),
            )),
        }
    }

    fn fail(&self, message: &str) -> AppError {
        self.transition(UpdatePhase::Error, message);
        AppError::Config(message.into())
    }
}

fn require_candidate(data: &super::state::UpdateData) -> AppResult<&PendingUpdate> {
    data.pending
        .as_ref()
        .ok_or_else(|| AppError::Config("Сначала проверьте наличие новой версии.".into()))
}

fn finish_install(
    service: &UpdateService,
    install: impl FnOnce() -> Result<(), ()>,
) -> AppResult<()> {
    // A panicking installer callback must finalize the UI and keep the
    // reservation sealed if shutdown had already started.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(install)) {
        Ok(Ok(())) => Ok(()),
        _ if service.shutdown_started.load(Ordering::Acquire) => Err(service.fail(
            "Установщик не запустился после завершения компонентов. Перезапустите Fono и повторите установку.",
        )),
        _ => Err(service.fail(
            "Не удалось запустить проверенный установщик. Повторите установку позже.",
        )),
    }
}

fn secure_plugin_config(config: Option<&serde_json::Value>) -> bool {
    let Some(config) = config else {
        return false;
    };
    config
        .get("requireSignedVersion")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && [
            "dangerousInsecureTransportProtocol",
            "dangerousAcceptInvalidCerts",
            "dangerousAcceptInvalidHostnames",
        ]
        .iter()
        .all(|field| config.get(*field).and_then(serde_json::Value::as_bool) != Some(true))
}

#[cfg(test)]
mod tests;

fn ensure_idle(app: &AppHandle) -> AppResult<()> {
    let pipeline = app.state::<crate::pipeline::Pipeline>();
    if pipeline.current_operation().is_some() || pipeline.has_active_service_operation() {
        return Err(AppError::Busy(
            "Сначала завершите текущую диктовку или задачу распознавания.".into(),
        ));
    }
    if crate::application::wake_calibration::status(app).active
        || crate::application::wake_validation::status(app).active
    {
        return Err(AppError::Busy(
            "Сначала завершите настройку или проверку фразы пробуждения.".into(),
        ));
    }
    let queue = app
        .state::<crate::application::service_control::ServiceControl>()
        .snapshot()
        .snapshot
        .queue;
    if queue.queued + queue.preparing + queue.transcribing > 0 {
        return Err(AppError::Busy(
            "Сначала завершите или отмените задачи API-сервиса.".into(),
        ));
    }
    if crate::application::models::has_active_downloads() {
        return Err(AppError::Busy(
            "Сначала завершите или отмените загрузку моделей.".into(),
        ));
    }
    Ok(())
}
