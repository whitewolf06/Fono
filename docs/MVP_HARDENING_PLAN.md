# Fono: working MVP baseline and hardening plan

## Baseline: `mvp-working-2026-07-10`

Это рабочий Windows MVP, а не прототип и не релиз для широкого
распространения. Подтверждённый основной сценарий:

```text
hotkey / wake word -> запись -> локальный Whisper -> опциональный LM Studio
-> вставка текста в активное окно
```

На этой контрольной точке также работают: tray, overlay, выбор микрофона и
модели, CUDA при специальной сборке, диагностика wake word, запись/проверка
wake-фразы и базовые команды после явного префикса «команда».

Wake word остаётся экспериментальной функцией: Whisper backend лучше подходит
для текущей пользовательской фразы, Sherpa-ONNX быстрее, но менее надёжен на
произвольном произношении.

## Что стабилизировали до baseline

- `fono-wake` отделён от основного pipeline и имеет backend-интерфейс.
- Wake flow загружает основной Whisper заранее, корректно возвращается из
  ошибок и возобновляет detector.
- Добавлены VAD-гистерезис, регулируемая пауза до перевода и таймер overlay.
- Устранена потеря границ фразы после wake word: pre-roll сохраняет начало,
  а повторный агрессивный VAD-trim не применяется к post-wake записи.
- Добавлены локальные команды после явного префикса «команда».

## P0 — надёжный дистрибутив и ядро диктовки

1. **Portable CPU release.**
   - [x] CPU-сборка без CUDA и без `target-cpu=native` — основной артефакт.
   - CUDA — отдельный optional build с явной маркировкой и CPU fallback.
   - MSI и NSIS проверить в чистой Windows VM без Rust, CUDA и исходников.
   - Проверить размещение Sherpa/ONNX DLL рядом с executable либо настроить
     корректный DLL search path до загрузки backend-а.
   - Зафиксировать один package manager: сейчас `npm` в Tauri config и
     `pnpm-lock.yaml`/локальный pnpm конфликтуют при пакетной сборке.

2. **Один владелец lifecycle диктовки.**
   - Выделить `DictationCoordinator` с operation ID, источником запуска,
     FSM и гарантированным cleanup.
   - Убрать копипасту capture -> transcribe -> process -> inject из hotkey,
     wake word и command flow.

3. **Настоящая отмена.**
   - [x] Проверять cancellation между стадиями и перед injection через
     operation ID; результат устаревшей операции не вставляется и не исполняет
     команду.
   - Отменять сам запрос к LLM, где это поддерживает провайдер.
   - Не показывать действие Stop там, где операция фактически неотменяема.

4. **Приватность и надёжная вставка.**
   - [x] Не записывать диктуемый текст, LLM prompt или ответ LLM в логи, даже
     при включённой подробной отладке.
   - Хранить API key в Windows Credential Manager; renderer получает только
     `has_api_key`.
   - Полные транскрипты логировать лишь в явном diagnostic mode, добавить
     retention/лимит размера логов.
   - [x] Проверить пары Unicode key-down/key-up, частичную отправку SendInput и
     сериализовать clipboard injection.

5. **Единый audio owner.**
   - Один WASAPI stream с fan-out: KWS, активная диктовка и диагностика.
   - Убрать ручной `unsafe Send + Sync` для CPAL stream и дублирование
     capture/resampling.

## P1 — доведение личного MVP

- Интерактивный first-run: микрофон -> модель -> тест -> hotkey -> готово.
- Экран готовности системы с конкретными способами исправить проблему.
- Разделить `Settings.tsx` на feature-секции и согласовать draft/apply.
- Синхронизировать README, STATUS, architecture и roadmap с реальным кодом.
- CI: format, clippy, Rust/TypeScript tests, production build и release smoke.

## P2 — Command Agent и развитие

После P0 Command Agent через LM Studio получает строгий allowlist tools, а не
доступ к shell или произвольным Windows API:

- `set_volume(percent: 0..100)` через Core Audio;
- `change_volume(delta)`, media controls;
- `open_app(name)` только из allowlist;
- `focus_window(query)` только среди видимых окон;
- подтверждение для рискованных действий.

Обычная диктовка не интерпретируется как команда: агент запускается только
после «команда …» или отдельной горячей клавиши.

## Отложенные улучшения

- streaming-транскрипция;
- профили приложений и контекстное окно;
- история транскриптов только opt-in;
- updater, подпись кода, i18n и другие ОС.
