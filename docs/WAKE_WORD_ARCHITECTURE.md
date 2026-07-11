# Архитектура wake word

Wake word изолирован в `src-tauri/crates/fono-wake`. Основной pipeline не
зависит от него: hotkey-диктовка остаётся доступной при любой ошибке wake word.

```text
audio input → fono-wake → Detected(pre-roll)
    → pause listener → post-wake dictation → STT → optional LLM → injection
    → resume listener
```

## Backends

### Whisper Experimental

Распознаёт короткий фрагмент Whisper-моделью и сравнивает фразу fuzzy matcher.
Это рекомендуемый вариант для фразы, которую пользователь меняет в UI. Минус:
выше задержка и нагрузка.

### Sherpa-ONNX

Быстрый KWS на модели
`sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`. Модель английская и
работает по BPE-токенам, а не по свободной строке. В текущем продукте проверены
карты `hey fono` и `okay fun`; остальные фразы нельзя обещать как рабочие, пока
не появится runtime BPE-tokenizer.

Диагностика Sherpa:

- `Эталон Sherpa WAV` проверяет модель на встроенной фразе `LIGHT UP`.
- Ручная запись/распознавание проверяет текущую wake-фразу и общий audio path.
- `—` означает «KWS не выдал keyword», а не текстовую транскрипцию.

## Инварианты lifecycle

- Listener ставится на паузу при диктовке, ручном тесте и отмене.
- Pre-roll сохраняет первые слова после wake phrase.
- После любой terminal state listener возобновляется, если wake word включён.
- Ошибка backend-а отправляется в UI/логи и не отключает hotkey-диктовку.

## Следующее улучшение

Добавить BPE-tokenizer для Sherpa в runtime либо ограничить UI списком
поддерживаемых фраз. До этого Whisper Experimental остаётся честным вариантом
для произвольного wake word.
