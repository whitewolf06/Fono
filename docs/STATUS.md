# Статус Fono

**Состояние:** V3 интегрирован с native runtime; обновление голосового ядра
2026-10-03 готовится к пользовательской проверке.

## Обновление голосового ядра

Локальные RU/EN фразы с мастер-проверкой, общий AudioHub, Silero VAD,
диктовка «На лету», безопасная пауза вставки при смене поля, отменяемые
workers protocol 3 и приоритет диктовки перед API реализованы.
Короткие публичные WAV и автоматические lifecycle-тесты проверены;
акустическая точность, многочасовые negatives, Windows поля и длительные
записи ещё требуют приёмки. Холодный Vulkan пока имеет заметную задержку.
Подробности: [fono-voice-reliability.md](fono-voice-reliability.md).

## Готово

- Hotkey-диктовка, VAD, отмена операции, overlay и безопасная text injection.
- Wake word с post-wake диктовкой, pre-roll и настраиваемой паузой тишины.
- Whisper Experimental wake word и Sherpa-ONNX KWS с download/test diagnostics.
- LM Studio/OpenAI-compatible post-processing: off, clean, format, command.
- Локальные команды: громкость, медиа, запуск allowlisted приложений, фокус окна.
- Выбор STT acceleration: Auto/CUDA/Vulkan/CPU.
- Отдельные CUDA/Vulkan worker'ы с preload модели и JSON Lines protocol.
- Локальный loopback REST API: bearer-authenticated health, asynchronous jobs и
  multipart-загрузка WAV/MP3/FLAC/OGG без доступа из сети.
- Проверены CUDA и Vulkan на NVIDIA RTX 5070 Ti; Vulkan использует актуальный
  upstream `whisper.cpp`.

## Release-проверки, которые ещё нужны

1. Собрать NSIS/MSI и проверить установленное приложение, а не только EXE из
   `target\release`.
2. Проверить Vulkan на AMD и Intel GPU.
3. Проверить Auto fallback без NVIDIA/CUDA и без Vulkan driver.
4. Прогнать first-run и hotkey/wake сценарии на чистой Windows VM.

## Подтверждённый API smoke (desktop dev)

В desktop debug runtime подтверждены: authenticated `GET /v1/health`,
`POST /v1/transcriptions` с WAV возвращает job `202`, `cancel` возвращает
`200`, а после штатной остановки Fono loopback-порт закрыт. Это не является
installer acceptance; инструкция для клиентов — в `LOCAL_TRANSCRIPTION_API.md`.

## Ограничения

- Прежний Sherpa-ONNX KWS не является свободным распознаванием речи. Для него доступны только
  фразы с корректной BPE-картой; сейчас это `hey fono` и `okay fun`.
- Whisper Experimental гибче для пользовательской фразы, но тяжелее и менее
  предсказуем по задержке.
- Command Agent через LM Studio ещё не реализован: сейчас доступны только
  локальные правила и allowlist-команды.

## Следующая продуктовая цель

Подтвердить качество и стабильность основного голосового ввода на реальном
голосе и Windows-приложениях; измерить задержку и ложные пробуждения на corpus.
Installer/clean-machine acceptance остаётся отдельным уровнем проверки.
