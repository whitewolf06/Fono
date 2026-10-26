# Архив плана улучшений FONO-47

Отчёты и измерения относятся к указанным в них версиям и условиям.
Текущий срез находится в [STATUS](../../STATUS.md), пользовательские инструкции —
в [руководстве](../../user-guide.md), оставшаяся работа — в [roadmap](../../roadmap.md).

## Этапы и общая приёмка

- [FONO-47 — общий dev для оставшейся приёмки](improvement-final-dev.md)
- [FONO-112 — исправления по ручной Windows-приёмке](improvement-fono112.md)
- [Первый этап improvement-plan](improvement-stage-1.md)
- [Второй этап improvement-plan](improvement-stage-2.md)
- [Третий этап improvement-plan](improvement-stage-3.md)
- [Четвёртый этап improvement-plan](improvement-stage-4.md)
- [Пятый этап improvement-plan](improvement-stage-5.md)

## Задачи FONO

- [FONO-117: выбор моделей и фактические метаданные](improvement-fono117.md)
- [FONO-118 — квантованные Whisper](improvement-fono118.md)
- [FONO-119 — сравнение CPU-движков](improvement-fono119.md)
- [FONO-120 — закрытые сегменты речи](improvement-fono120.md)
- [FONO-121 — фоновые сегменты и scheduler](improvement-fono121.md)
- [FONO-122 — Stop: префикс, хвост и один итог](improvement-fono122.md)
- [FONO-123 — настройка предварительного распознавания](improvement-fono123.md)
- [FONO-124 — проверка длинной диктовки](improvement-fono124.md)
- [FONO-125 — триггеры клавиатуры и мыши](improvement-fono125.md)
- [FONO-126 — управление работающим Fono через CLI](improvement-fono126.md)
- [FONO-127 — выбор кнопки диктовки и проверка конфликтов](improvement-fono127.md)
- [FONO-128 — явное получение выделения](improvement-fono128.md)
- [FONO-129 — преобразование выделения по инструкции](improvement-fono129.md)
- [FONO-130 — безопасная замена выделения](improvement-fono130.md)
- [FONO-131 — overlay правки и приватность](improvement-fono131.md)
- [FONO-132 — приоритет терминов по использованию](improvement-fono132-priority.md)
- [FONO-132 — личный словарь в STT и LLM](improvement-fono132.md)
- [FONO-133 — профили приложений](improvement-fono133.md)
- [FONO-134 — транскрибация файлов](improvement-fono134.md)
- [FONO-135 — локальные сниппеты](improvement-fono135.md)
- [FONO-136 — язык распознавания в overlay](improvement-fono136.md)
- [FONO-137 — русский и английский интерфейс](improvement-fono137.md)
- [FONO-138 — автостоп по тишине в toggle](improvement-fono138.md)
- [FONO-139 — темп речи и паузы в периодном тренере](improvement-fono139.md)
- [FONO-140 — звуковые сигналы](improvement-fono140.md)
- [FONO-141 — локальные подключения Ollama и LM Studio](improvement-fono141.md)
- [FONO-142 — повторная проверка замороженного WakeWord](improvement-fono142.md)
- [FONO-143 — отдельный стенд оценки WakeWord](improvement-fono143.md)
- [FONO-144 — подготовка обучения фиксированной WakeWord-фразы](improvement-fono144.md)
- [FONO-27 — ручная редактура архивной записи](improvement-fono27.md)
- [FONO-28 — контекст вставки и статистика приложений](improvement-fono28.md)

## PERF-04

- [PERF-04: результаты проверки ресемплера](improvement-perf04.md)

## Пробуждение и Лира

- [WakeWord: реальные образцы и первый классификатор](improvement-wake-first-candidate.md)
- [«Привет, Лира»: ложные события без новых пропусков — v0.7.71](improvement-wake-lira-ctc.md)
- [«Привет, Лира»: ручная проверка в dev — v0.7.72](improvement-wake-lira-dev.md)
- [Проверка «Привет, Лира» — v0.7.70](improvement-wake-lira-evaluation.md)
- [«Привет, Лира»: живое пробуждение в dev](improvement-wake-lira-live.md)
- [Запись «Привет, Лира» — v0.7.68](improvement-wake-lira.md)
- [WakeWord: локальная запись образцов](improvement-wake-samples.md)

## Исходные данные

JSON-снимки сохранены в `evidence/` без изменения байтов. Ссылки из отчётов
ведут к соответствующим данным. Встроенные в JSON пути и статусы исторические.
Notices моделей и корпусов остаются в [evidence/licenses](../../evidence/licenses/).

Новые результаты рабочих проверок записывайте в `build/`; критерии и исходную
базу сравнения нельзя менять переносом или переименованием старого отчёта.
