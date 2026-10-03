use super::{
    model::{source_label, ReportSnapshot},
    telemetry::DictationMeasurement,
};
use std::fmt::Write;

fn yes(value: bool) -> &'static str {
    if value {
        "да"
    } else {
        "нет"
    }
}
fn available(value: Option<bool>) -> &'static str {
    value.map(yes).unwrap_or("не удалось проверить")
}
fn milliseconds(value: Option<u64>) -> String {
    value
        .map(|ms| format!("{ms} мс"))
        .unwrap_or_else(|| "не измерено".into())
}

pub(super) fn render(snapshot: &ReportSnapshot, last: Option<&DictationMeasurement>) -> String {
    let s = snapshot;
    let mut text = String::from("Fono — локальный отчёт диагностики (схема 1)\n");
    let _ = writeln!(
        text,
        "Версия: {} | Ревизия: {} | Сборка: {}",
        s.version, s.revision, s.profile
    );
    let _ = writeln!(
        text,
        "Платформа: {} / {} | Runtime: Tauri / Rust",
        s.os, s.arch
    );
    let _ = writeln!(
        text,
        "Состояние: {:?} | Диктовка приостановлена: {} | Режим: классический",
        s.phase,
        yes(s.paused)
    );
    let _ = writeln!(text, "\nРаспознавание");
    let _ = writeln!(
        text,
        "Модель: {} | Файл доступен: {} | Язык: {}",
        s.model,
        yes(s.model_available),
        s.language
    );
    let _ = writeln!(
        text,
        "Ускорение: {:?} | Активный backend: {} | STT: {}",
        s.acceleration, s.active_backend, s.stt_state
    );
    let _ = writeln!(
        text,
        "Доступные сборки: CPU; CUDA: {}; Vulkan: {} | Протокол worker: {}",
        yes(s.cuda_available),
        yes(s.vulkan_available),
        fono_stt_protocol::PROTOCOL_VERSION
    );
    let _ = writeln!(
        text,
        "Наличие GPU-сборки не подтверждает совместимость драйвера или GPU."
    );
    let _ = writeln!(text, "\nАудио и активация");
    let _ = writeln!(
        text,
        "Выбор микрофона: {} | Доступен: {} | Число входов: {}",
        if s.microphone_default {
            "системный"
        } else {
            "выбранный вручную"
        },
        available(s.microphone_available),
        s.input_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "не измерено".into())
    );
    let _ = writeln!(
        text,
        "Потеря пакетов за текущий процесс: захват {}; подписчики {} | Ошибка AudioHub: {}",
        s.capture_dropped,
        s.subscriber_dropped,
        yes(s.audio_error)
    );
    let _ = writeln!(
        text,
        "WakeWord: {} | Backend: {:?} | Состояние: {:?} | Профиль калибровки: {}",
        yes(s.wake_enabled),
        s.wake_backend,
        s.wake_status,
        yes(s.wake_calibrated)
    );
    let _ = writeln!(text, "\nФункции");
    let _ = writeln!(
        text,
        "Обработка: {:?} | Вставка: {:?}",
        s.processing, s.injection
    );
    let _ = writeln!(
        text,
        "Подключение обработки: {} | Модель обработки выбрана: {}",
        match s.processing_connection {
            Some(crate::types::LlmConnectionKind::Local) => "локальное",
            Some(crate::types::LlmConnectionKind::Cloud) => "облачное",
            None => "не назначено",
        },
        yes(s.processing_model_configured)
    );
    let _ = writeln!(
        text,
        "Личный словарь включён: {} | Записей: {}",
        yes(s.dictionary_enabled),
        s.dictionary_count
    );
    let _ = writeln!(
        text,
        "История: {} | Тренер: {} | Подробный журнал: {}",
        yes(s.history_enabled),
        yes(s.trainer_enabled),
        yes(s.verbose_logging)
    );
    let _ = writeln!(
        text,
        "API: {} | Ошибка запуска: {}",
        yes(s.service_enabled),
        yes(s.service_error)
    );
    let [queued, preparing, transcribing, completed, failed, cancelled] = s.queue;
    let _ = writeln!(text, "Очередь API: ожидают {queued}; подготовка {preparing}; распознавание {transcribing}; завершены {completed}; ошибки {failed}; отменены {cancelled}");
    let _ = writeln!(
        text,
        "\nПоследняя завершённая классическая диктовка в текущем процессе"
    );
    if let Some(last) = last {
        let _ = writeln!(
            text,
            "Запуск: {} | Результат: {}",
            source_label(last.source),
            last.outcome.label()
        );
        let _ = writeln!(
            text,
            "Длительность захваченного аудио: {}",
            milliseconds(last.audio_ms)
        );
        let _ = writeln!(
            text,
            "После команды завершения до результата: {} мс",
            last.finish_ms
        );
        for (label, duration) in [
            ("Завершение захвата и подготовка", last.capture_finish_ms),
            ("Ожидание и загрузка модели", last.model_load_ms),
            ("Ожидание и распознавание", last.recognition_ms),
            ("Обработка текста", last.processing_ms),
            ("Вставка в поле", last.insertion_ms),
        ] {
            let _ = writeln!(text, "{label}: {}", milliseconds(duration));
        }
        let _ = writeln!(text, "Стадии включают ожидание планировщика. Время вставки не подтверждает приём текста внешним приложением.");
    } else {
        let _ = writeln!(
            text,
            "Нет измерений. Завершите обычную диктовку и создайте отчёт ещё раз."
        );
    }
    let _ = writeln!(text, "\nПриватность: отчёт не содержит текстов, аудио, словаря, фразы пробуждения, имён устройств, путей, ключей, адресов подключений, инструкций или журнала. Он создаётся локально; отправка не выполняется.");
    text
}
