use super::super::{state::cancelled, UpdatePhase, UpdateService};
use crate::error::AppResult;
use std::{sync::Arc, time::Duration};
use tauri_plugin_updater::{Error, Update};
use tokio::sync::watch;

pub(super) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

pub(super) async fn download(
    service: &UpdateService,
    update: &Update,
    cancellation: &Arc<watch::Sender<u8>>,
    receiver: &mut watch::Receiver<u8>,
) -> AppResult<Option<Vec<u8>>> {
    service.transition(
        UpdatePhase::Downloading,
        "Загружаем обновление и проверяем подпись…",
    );
    service.data.lock().snapshot.downloaded_bytes = 0;
    let result = tokio::select! {
        biased;
        reason = cancelled(receiver) => {
            if reason == 2 { return Err(service.fail("Пакет обновления превышает допустимый размер. Установка запрещена.")); }
            service.transition(UpdatePhase::Available, "Загрузка отменена. Установщик не запускался.");
            return Ok(None);
        },
        result = tokio::time::timeout(DOWNLOAD_TIMEOUT, update.download(|chunk, total| {
            let mut data = service.data.lock();
            data.snapshot.downloaded_bytes = data.snapshot.downloaded_bytes.saturating_add(chunk as u64);
            data.snapshot.total_bytes = total;
            if data.snapshot.downloaded_bytes > MAX_DOWNLOAD_BYTES || total.is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
                cancellation.send_replace(2);
            }
        }, || {})) => result,
    };
    match result {
        Err(_) => Err(service.fail("Загрузка заняла слишком много времени. Повторите позже.")),
        Ok(Err(error)) => {
            let message = match error {
                Error::Minisign(_)
                | Error::Base64(_)
                | Error::SignatureUtf8(_)
                | Error::SignedVersionMismatch { .. }
                | Error::MissingSignedVersion => {
                    "Подпись или версия пакета не прошла проверку. Установка запрещена."
                }
                _ => "Не удалось загрузить обновление. Проверьте подключение и повторите позже.",
            };
            Err(service.fail(message))
        }
        Ok(Ok(bytes)) => {
            if bytes.is_empty() || bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
                return Err(service.fail(
                    "Пакет обновления пуст или превышает допустимый размер. Установка запрещена.",
                ));
            }
            Ok(Some(bytes))
        }
    }
}
