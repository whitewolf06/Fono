use super::super::{
    automatic, configuration::is_public_https, settings_transaction, state::cancelled,
};
use super::*;
use std::time::Duration;
use tauri_plugin_updater::UpdaterExt;

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

impl UpdateService {
    pub(in crate::application::updates) async fn check(&self, app: &AppHandle) -> AppResult<()> {
        let _activity = activity::lease()?;
        let _serial = Arc::clone(&self.serial).try_lock_owned().map_err(|_| {
            AppError::Busy("Проверка или установка обновления уже выполняется.".into())
        })?;
        self.check_locked(app, false).await
    }

    pub(in crate::application::updates) async fn check_automatically(
        &self,
        app: &AppHandle,
        schedule: &mut automatic::Schedule,
    ) -> AppResult<bool> {
        let _activity = activity::lease()?;
        let _serial = Arc::clone(&self.serial).try_lock_owned().map_err(|_| {
            AppError::Busy("Проверка или установка обновления уже выполняется.".into())
        })?;
        let transaction = settings_transaction(app).await;
        if !app
            .state::<crate::state::AppState>()
            .settings()
            .update_checks_enabled
            || self.channel.is_none()
        {
            return Ok(false);
        }
        let now = automatic::unix_now();
        if !schedule.delay(now).is_zero() {
            return Ok(false);
        }
        // Count attempts, including failed requests, so restarts cannot flood
        // the feed. If persistence fails, skip networking and rate-limit in RAM.
        schedule.record_attempt(now);
        crate::state::save_update_check_time(now).map_err(|_| {
            self.fail(
                "Не удалось сохранить время автоматической проверки. Проверьте обновления вручную.",
            )
        })?;
        // Mark the automatic origin before allowing opt-out to run. It can
        // cancel this check without cancelling an explicit download/install.
        let result = self.begin_check(app, true)?;
        drop(transaction);
        self.finish_check(result).await?;
        Ok(true)
    }

    async fn check_locked(&self, app: &AppHandle, automatic: bool) -> AppResult<()> {
        if self.channel.is_none() {
            return Ok(());
        }
        let check = self.begin_check(app, automatic)?;
        self.finish_check(check).await
    }

    fn begin_check(
        &self,
        app: &AppHandle,
        automatic: bool,
    ) -> AppResult<(tauri_plugin_updater::Updater, watch::Receiver<u8>)> {
        let channel = self.channel.as_ref().ok_or_else(|| {
            AppError::Config("Канал подписанных обновлений ещё не настроен.".into())
        })?;
        if !secure_plugin_config(app.config().plugins.0.get("updater")) {
            return Err(self.fail("Проверка подписанной версии или безопасного TLS не настроена. Обновление отключено."));
        }
        let (sender, receiver) = watch::channel(0u8);
        let shutdown_app = app.clone();
        let shutdown_started = Arc::clone(&self.shutdown_started);
        let updater = app
            .updater_builder()
            .pubkey(channel.public_key.clone())
            .endpoints(vec![channel.endpoint.clone()])
            .map_err(|_| self.fail("Конфигурация подписанного канала некорректна."))?
            .timeout(CHECK_TIMEOUT)
            .version_comparator(|current, release| {
                release.version > current && release.version.pre.is_empty()
            })
            .configure_client(|builder| {
                builder
                    .https_only(true)
                    .connect_timeout(Duration::from_secs(10))
            })
            .on_before_exit(move || {
                shutdown_started.store(true, Ordering::Release);
                crate::shutdown_app(&shutdown_app);
                shutdown_app.cleanup_before_exit();
            })
            .build()
            .map_err(|_| self.fail("Не удалось подготовить проверку обновлений."))?;
        {
            let mut data = self.data.lock();
            data.snapshot.phase = UpdatePhase::Checking;
            data.snapshot.message = "Проверяем подписанный канал обновлений…".into();
            data.snapshot.downloaded_bytes = 0;
            data.snapshot.total_bytes = None;
            data.pending = None;
            data.snapshot.next_version = None;
            data.cancellation = Some(Arc::new(sender));
            data.automatic_check = automatic;
        }
        Ok((updater, receiver))
    }

    async fn finish_check(
        &self,
        (updater, mut receiver): (tauri_plugin_updater::Updater, watch::Receiver<u8>),
    ) -> AppResult<()> {
        let _action = ActionGuard(self);
        let checked = tokio::select! {
            biased;
            _ = cancelled(&mut receiver) => {
                self.transition(UpdatePhase::Idle, "Проверка обновлений отменена.");
                return Ok(());
            },
            checked = tokio::time::timeout(CHECK_TIMEOUT, updater.check()) => checked,
        };
        match checked {
            Err(_) => Err(self.fail("Проверка заняла слишком много времени. Повторите позже.")),
            Ok(Err(_)) => Err(self
                .fail("Не удалось проверить обновления. Проверьте подключение и повторите позже.")),
            Ok(Ok(None)) => {
                self.transition(UpdatePhase::UpToDate, "Установлена актуальная версия Fono.");
                Ok(())
            }
            Ok(Ok(Some(update))) => {
                if !is_public_https(&update.download_url) || update.signature.is_empty() {
                    return Err(self.fail(
                        "Канал вернул небезопасный или неподписанный пакет. Установка запрещена.",
                    ));
                }
                let mut data = self.data.lock();
                data.snapshot.phase = UpdatePhase::Available;
                data.snapshot.next_version = Some(update.version.clone());
                data.snapshot.message =
                    "Новая версия доступна. Установка начнётся только по вашей команде.".into();
                data.pending = Some(PendingUpdate {
                    update,
                    verified_bytes: None,
                });
                Ok(())
            }
        }
    }
}
