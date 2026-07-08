# 🎙️ WhisperClone

> Голосовой ввод для Windows по образцу [Wispr Flow](https://wisprflow.ai/) — **полностью локально и приватно**.

WhisperClone — это десктоп-приложение, которое превращает вашу речь в текст в **любом окне Windows**:
скажите ключевую фразу — и приложение начнёт слушать, расшифрует вашу речь через
локальный STT и при необходимости обработает её через локальный LLM (LM Studio),
а затем вставит результат в активное поле ввода.

## ✨ Возможности

- **Активация по ключевой фразе** (wake word) — как «Hey Siri»: приложение постоянно слушает
  микрофон лёгкой моделью и реагирует на вашу фразу (например, «Эй, ассистент»).
- **Push-to-talk** — резервный режим: зажмите глобальную горячую клавишу и диктуйте.
- **Локальная транскрипция** на [whisper.cpp](https://github.com/ggerganov/whisper.cpp) — 100% офлайн,
  поддержка 100+ языков (упор на русский и английский).
- **AI-постобработка** через [LM Studio](https://lmstudio.ai/) на `localhost:1234`:
  чистка оговорок («ээ», «мм»), пунктуация, форматирование, команды
  («преврати в email», «переведи на английский», «сделай короче»).
- **Вставка в любое окно** через Win32 `SendInput` — работает в браузере, Word, мессенджерах, IDE.
- **Минималистичный overlay-индикатор** статуса (слушаю / транскрибирую / готово).
- **System tray** — фоновая работа, быстрая пауза, выход, настройки.
- **Глобальная горячая клавиша** для запуска/остановки диктовки.

## 🔒 Приватность

Весь конвейер работает **локально на вашем компьютере**. Ни аудио, ни текст не покидают
машину (за исключением случая, когда вы явно подключите облачный LLM/STT — этого нет
в базовой конфигурации).

## 🧱 Стек технологий

| Слой | Технология |
|---|---|
| Каркас приложения | [Tauri 2](https://v2.tauri.app/) |
| Ядро (system, audio, FFI) | Rust |
| UI | React + TypeScript + Tailwind CSS |
| Захват аудио | [`cpal`](https://crates.io/crates/cpal) (WASAPI) |
| STT | [whisper.cpp](https://github.com/ggerganov/whisper.cpp) через FFI ([`whisper-rs`](https://crates.io/crates/whisper-rs)) |
| VAD (распознавание речи/тишины) | silero-vad |
| Wake word | openWakeWord / Porcupine / ONNX (см. `docs/architecture.md`) |
| LLM-постобработка | LM Studio (OpenAI-совместимый API) |
| Текст-инъекция | Win32 `SendInput` через крейт [`windows`](https://crates.io/crates/windows) |
| Tray / overlay / hotkey | `tauri-plugin-*` v2 |

## 📦 Установка (для конечных пользователей)

> ⚙️ Раздел будет заполнен после сборки первого инсталлятора (Этап 7).
> В久之时间内 используйте сборку из исходников по инструкции ниже.

## 🛠️ Сборка из исходников

См. подробную инструкцию в [`docs/development.md`](docs/development.md).

Кратко:

```bash
# 1. Установите Rust toolchain (https://rustup.rs) и Node.js 20+
rustc --version    # >= 1.75
node --version     # >= 20

# 2. Установите зависимости frontend
npm install

# 3. Запустите dev-режим
npm run tauri dev

# 4. Соберите инсталлятор
npm run tauri build
```

Также потребуется:
- [LM Studio](https://lmstudio.ai/) с загруженной моделью (рекомендуется Qwen2.5-12B или Llama-3-8B),
  запущенной как локальный сервер на `http://localhost:1234`.
- Whisper-модели в формате GGML (`ggml-base.bin`, `ggml-medium.bin`) — скачиваются при первом запуске
  или кладутся вручную в `src-tauri/resources/whisper/`.

## 📐 Архитектура

Подробное описание модулей и потоков данных — в [`docs/architecture.md`](docs/architecture.md).

```
┌─────────────────────────────────────────────────────────┐
│  Tauri 2 App (Rust core + Webview UI на React/Svelte)   │
├─────────────────────────────────────────────────────────┤
│  Frontend (TypeScript)                                  │
│   - Settings UI, overlay, tray menu, onboarding         │
├─────────────────────────────────────────────────────────┤
│  Rust Core                                              │
│   1. Audio Capture (cpal, ring buffer)                  │
│   2. Wake Word  (openWakeWord / Porcupine)              │
│   3. VAD        (silero-vad)                            │
│   4. STT        (whisper.cpp FFI)                       │
│   5. LLM        (HTTP → LM Studio localhost:1234)       │
│   6. Injection  (windows crate: SendInput)              │
│   7. Hotkey     (tauri-plugin-global-shortcut)          │
│   8. Tray/Overlay                                    │
└─────────────────────────────────────────────────────────┘
```

## 🗺️ Roadmap

Подробный план с контрольными точками — в [`docs/roadmap.md`](docs/roadmap.md).
Актуальный статус — в [`docs/STATUS.md`](docs/STATUS.md).

**Версия: v0.4.0-wakeword** — рабочий MVP.

- [x] Этап 0 — Документация + скаффолд
- [x] Этап 1 — Audio capture + whisper.cpp STT
- [x] Этап 2 — Текст-инъекция через SendInput
- [x] Этап 3 — Push-to-talk + VAD 🎯 *(рабочий голосовой ввод)*
- [x] Оптимизация CPU (AVX2/FMA, small = 3 сек вместо 34 сек)
- [x] Этап 4 — Wake word «Эй, ассистент»
- [ ] Этап 5 — AI-постобработка через LM Studio
- [ ] Этап 6 — UX polish, настройки, onboarding
- [ ] Этап 7 — Упаковка (MSI/NSIS), подпись кода, автообновление

## 📄 Лицензия

TBD (предположительно MIT или Apache-2.0).

## 🙏 Благодарности

- [Wispr Flow](https://wisprflow.ai/) — за вдохновение и референс UX.
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (Georgi Gerganov) — за лучший локальный STT.
- [Tauri](https://tauri.app/) — за прекрасный каркас для лёгких десктоп-приложений.
