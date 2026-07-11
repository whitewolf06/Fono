# Статус Fono

**Состояние:** рабочий Windows MVP, ветка после `mvp-working-2026-07-10`.

## Готово

- Hotkey-диктовка, VAD, отмена операции, overlay и безопасная text injection.
- Wake word с post-wake диктовкой, pre-roll и настраиваемой паузой тишины.
- Whisper Experimental wake word и Sherpa-ONNX KWS с download/test diagnostics.
- LM Studio/OpenAI-compatible post-processing: off, clean, format, command.
- Локальные команды: громкость, медиа, запуск allowlisted приложений, фокус окна.
- Выбор STT acceleration: Auto/CUDA/Vulkan/CPU.
- Отдельные CUDA/Vulkan worker'ы с preload модели и JSON Lines protocol.
- Проверены CUDA и Vulkan на NVIDIA RTX 5070 Ti; Vulkan использует актуальный
  upstream `whisper.cpp`.

## Release-проверки, которые ещё нужны

1. Собрать NSIS/MSI и проверить установленное приложение, а не только EXE из
   `target\release`.
2. Проверить Vulkan на AMD и Intel GPU.
3. Проверить Auto fallback без NVIDIA/CUDA и без Vulkan driver.
4. Прогнать first-run и hotkey/wake сценарии на чистой Windows VM.

## Ограничения

- Sherpa-ONNX — KWS, не свободное распознавание речи. Для него надёжны только
  фразы с корректной BPE-картой; сейчас это `hey fono` и `okay fun`.
- Whisper Experimental гибче для пользовательской фразы, но тяжелее и менее
  предсказуем по задержке.
- Command Agent через LM Studio ещё не реализован: сейчас доступны только
  локальные правила и allowlist-команды.

## Следующая продуктовая цель

Сначала завершить installer/clean-machine acceptance и first-run diagnostics,
затем добавить безопасный Command Agent с ограниченным набором инструментов.
