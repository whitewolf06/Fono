use std::sync::mpsc::TryRecvError;

use super::{
    controls::{set_text, Controls},
    worker::{self, Download, Event},
};
use crate::install::Outcome;
use fono_setup::{Phase, Progress};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{Controls::*, WindowsAndMessaging::*},
};

#[derive(Default)]
pub(super) struct App {
    pub(super) controls: Option<Controls>,
    pub(super) download: Option<Download>,
    close_when_finished: bool,
    cancelling: bool,
    pub(super) startup_error: Option<windows::core::Error>,
}
impl App {
    pub(super) unsafe fn start(&mut self) {
        self.close_when_finished = false;
        self.cancelling = false;
        if let Some(controls) = &self.controls {
            set_text(controls.status, "Проверяем доступную версию…");
            set_text(controls.details, "Соединяемся с GitHub Releases. При обрыве загрузку можно повторить: уже скачанная часть остаётся в кэше.");
            controls.actions("Загрузка…", false, "Отмена", true);
            SendMessageW(controls.progress, PBM_SETPOS, WPARAM(0), LPARAM(0));
        }
        self.download = Some(worker::start());
    }

    pub(super) unsafe fn close_or_cancel(&mut self, window: HWND, close: bool) {
        if let Some(download) = &self.download {
            self.close_when_finished |= close;
            if download.request_cancel() {
                self.cancelling = true;
                if let Some(controls) = &self.controls {
                    set_text(controls.status, "Останавливаем загрузку…");
                    controls.actions("Загрузка…", false, "Отмена…", false);
                }
            } else if let Some(controls) = &self.controls {
                set_text(controls.status, "Завершите открытый мастер установки");
            }
        } else {
            let _ = DestroyWindow(window);
        }
    }

    pub(super) unsafe fn receive(&mut self, window: HWND) {
        loop {
            let event = match self
                .download
                .as_ref()
                .map(|download| download.events.try_recv())
            {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Disconnected)) => Event::Finished(Err(
                    "Загрузка неожиданно прервалась. Можно повторить попытку.".into(),
                )),
                _ => break,
            };
            match event {
                Event::Progress(progress) => self.progress(progress),
                Event::Installing(version) => {
                    if let Some(controls) = &self.controls {
                        set_text(controls.status, "Открываем мастер установки…");
                        set_text(controls.details, &format!("Fono {version}: подпись проверена. Подтвердите запрос Windows на изменение устройства и пройдите шаги мастера. Загрузчик дождётся его завершения."));
                        controls.actions("Установка…", false, "Закрыть", false);
                    }
                }
                Event::Finished(result) => {
                    self.download = None;
                    if self.close_when_finished {
                        let _ = DestroyWindow(window);
                        break;
                    }
                    if let Some(controls) = &self.controls {
                        match result {
                            Ok(Outcome::Completed) => {
                                set_text(controls.status, "Мастер установки завершён");
                                set_text(controls.details, "Установщик сообщил об успешном завершении. Fono можно запустить через меню «Пуск». Загрузчик можно закрыть.");
                                controls.actions("Установлено", false, "Закрыть", true);
                            }
                            Ok(Outcome::Cancelled) => {
                                set_text(controls.status, "Установка отменена");
                                set_text(controls.details, "Загруженные данные сохранены. Нажмите «Повторить», чтобы продолжить; проверенный пакет повторно скачивать не потребуется.");
                                controls.actions("Повторить", true, "Закрыть", true);
                            }
                            Err(error) => {
                                set_text(controls.status, "Не удалось завершить установку");
                                set_text(controls.details, &error);
                                controls.actions("Повторить", true, "Закрыть", true);
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    unsafe fn progress(&self, progress: Progress) {
        if self.cancelling {
            return;
        }
        let Some(controls) = &self.controls else {
            return;
        };
        if let Some(version) = &progress.version {
            set_text(
                controls.version,
                &format!(
                    "Fono {version} · загрузчик {}",
                    fono_setup::config::bootstrap_version()
                ),
            );
        }
        let status = match progress.phase {
            Phase::Resolving => "Проверяем доступную версию…",
            Phase::Downloading => "Скачиваем Fono…",
            Phase::Verifying => "Проверяем цифровую подпись…",
            Phase::Ready => "Пакет скачан и проверен",
        };
        set_text(controls.status, status);
        if progress.phase == Phase::Downloading {
            let downloaded = progress.downloaded_bytes as f64 / 1_048_576.0;
            let details = match progress.total_bytes {
                Some(total) => {
                    let position = (u128::from(progress.downloaded_bytes) * 1000 / u128::from(total.max(1))).min(1000) as usize;
                    SendMessageW(controls.progress, PBM_SETPOS, WPARAM(position), LPARAM(0));
                    format!("Скачано {downloaded:.1} из {:.1} МБ. Загрузка сохраняется для продолжения после обрыва.", total as f64 / 1_048_576.0)
                }
                None => format!("Скачано {downloaded:.1} МБ. Загрузка сохраняется для продолжения после обрыва."),
            };
            set_text(controls.details, &details);
        } else if progress.phase == Phase::Verifying {
            SendMessageW(controls.progress, PBM_SETPOS, WPARAM(1000), LPARAM(0));
            set_text(controls.details, "Проверяем подпись издателя и подписанную версию. Непроверенный файл не будет запущен.");
        }
    }
}
