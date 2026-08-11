# Архитектура Fono

Этот документ описывает модульную структуру приложения, потоки данных и
ключевые технические решения на текущий момент.

## 1. Высокоуровневая схема

```
┌──────────────────────────────────────────────────────────────────┐
│                       Tauri 2 Application                        │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  Frontend (Webview, React + TypeScript + Tailwind)         │  │
│  │  - Settings, Overlay, Onboarding (cosmetic)                │  │
│  │  - IPC через tauri::invoke / Tauri events                  │  │
│  └─────────────────────────────┬──────────────────────────────┘  │
│                                │  invoke / events                │
│  ┌─────────────────────────────┴──────────────────────────────┐  │
│  │                     Rust Core (главный процесс)            │  │
│  │                                                            │  │
│  │  ┌─────────────┐    ┌──────────────┐                      │  │
│  │  │   audio/    │───▶│   wakeword/  │                      │  │
│  │  │  cpal       │    │ whisper-base │                      │  │
│  │  └─────┬───────┘    └──────┬───────┘                      │  │
│  │        │                   │                              │  │
│  │        │     ┌─────────────────────────┐                  │  │
│  │        ▼     │                         │                  │  │
│  │      ┌──────────────┐     pipeline/ (оркестратор)         │  │
│  │      │    vad/      │◀──── start/stop dictation           │  │
│  │      │ trim_silence │                  ▲                  │  │
│  │      └─────┬────────┘                  │                  │  │
│  │            ▼                           │                  │  │
│  │      ┌──────────────┐                  │                  │  │
│  │      │    stt/      │                  │                  │  │
│  │      │ whisper.cpp  │◀── global Ctrl+Space (tauri-plugin) │  │
│  │      └──────┬───────┘                  │                  │  │
│  │             │                          │                  │  │
│  │             ▼                          │                  │  │
│  │      ┌──────────────┐                  │                  │  │
│  │      │    llm/      │                  │                  │  │
│  │      │ LM Studio    │                  │                  │  │
│  │      └──────┬───────┘                  │                  │  │
│  │             │                          │                  │  │
│  │             ▼                          │                  │  │
│  │      ┌──────────────┐                  │                  │  │
│  │      │  injection/  │──────────────────┘                  │  │
│  │      │ SendInput or │                                     │  │
│  │      │ Clipboard    │                                     │  │
│  │      └──────────────┘                                     │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

## 2. Основной конвейер (Pipeline)

Конвейер — это конечный автомат с состояниями:

```
        ┌──────────┐
        │  IDLE    │  ◀── фоновый wake word (если включён)
        └────┬─────┘
   wake word  │   или глобальная горячая клавиша
   обнаружена │
             ▼
        ┌──────────┐
        │ LISTENING│  ◀── запись аудио в буфер
        └────┬─────┘
   отпускание │   или тишина (только wake word)
   клавиши     │
             ▼
        ┌──────────┐
        │TRANSCRIB-│  ◀── whisper.cpp декодирует буфер
n        │  ING     │
        └────┬─────┘
             ▼
        ┌──────────┐
        │PROCESSING│  ◀── (опционально) LLM-постобработка
        └────┬─────┘
             ▼
        ┌──────────┐
        │ INJECTING│  ◀── SendInput или Clipboard (Ctrl+V)
        └────┬─────┘
             │
             ▼
        ┌──────────┐
        │   IDLE   │
        └──────────┘
```

### События для UI

Backend публикует versioned envelope `backend-event-v1` (`schema_version = 1`,
типизированный payload и `operation_id`). Legacy-события, включая
`pipeline-state`, пока формируются compatibility mapper'ом для UI v2.

## 3. Описание модулей

### `audio/` — захват аудио
- Использует крейт `cpal` (WASAPI на Windows).
- Захватывает выбранный микрофон на **16 кГц, моно, i16**.
- Конвертирует любой входной формат (i8/i16/i32/i64/u8/u16/u32/u64/f32/f64) в i16.
- CPAL stream принадлежит выделенному owner thread внутри `AudioHub` и не
  передаётся между потоками через unsafe wrapper.
- Callback переиспользует conversion/mix/resample buffers; уровень последних
  frames публикуется через atomic, без сканирования recording buffer.
- `AudioHub` открывает один physical stream для выбранного device/rate и
  выдаёт bounded RAII subscriptions wake и dictation. При wake → dictation
  wake только ставится на pause, а устройство не закрывается и не открывается
  заново.

### Operation resources

`fono-core::OperationCoordinator` владеет operation-scoped leases `audio`,
`stt`, `injection` и `command_proposal`. Pipeline переводит lease вместе со
стадией операции, а terminal transition удаляет весь набор даже при cancel или
fault. Audio и overlay adapter boundaries имеют автоматический fault-injection;
это не заменяет ручную проверку реального устройства и окна.

### `wakeword/` — детекция ключевой фразы
- Каждый backend публикует capabilities. Sherpa-ONNX поддерживает только
  `hey fono` и `okay fun`; Whisper Experimental допускает произвольную phrase.
  Неподдерживаемая Sherpa phrase отклоняется до сохранения settings.
- Фоновый поток, который каждые ~1.5 сек берёт чанк аудио.
- VAD-gating: тихие чанки пропускаются.
- Транскрибирует чанк моделью `ggml-base.bin`.
- Нечёткий поиск фразы через расстояние Левенштейна.
- Cooldown между срабатываниями.
- При срабатывании запускает post-wake диктовку с VAD-остановкой.

### `vad/` — Voice Activity Detection
- Энергетический VAD с порогом ~-38 dBFS.
- `trim_silence` обрезает тишину в начале/конце записи.
- Используется только для обрезки и для wake-word диктовки.

### `stt/` — Speech-to-Text
- Биндинги к whisper.cpp через `whisper-rs` 0.16.
- Ленивая загрузка модели (`ensure_loaded`).
- Поддержка моделей: `tiny`, `base`, `small`, `medium`, `large-v3`.
- GPU-ускорение: runtime-флаг `use_gpu` передаётся в `WhisperContextParameters`.
- Возвращает `Transcript` с текстом, языком, временем обработки и устройством (CPU/CUDA).
- Routing state удерживается только при выборе или замене backend-а; длительная
  транскрипция выполняется под отдельным lock выбранной embedded/worker session.
  `SttReadiness` отдельно публикует `unloaded/loading/ready/failed` через IPC.
  `SttHealth` использует non-blocking `try_lock`: `busy` не ждёт inference, а
  свободный worker подтверждается protocol-v2 `ping`. Cancellation прерывает
  ожидание ответа worker и завершает его session; для embedded Whisper остаются
  безопасные проверки до/после native inference и operation fencing. Это не
  заменяет будущие actor/mailbox. После Tauri setup выбранная сохранённая модель
  готовится в фоне; ошибка preload отражается в readiness, а не ломает startup.

### `llm/` — AI-постобработка
- HTTP-клиент (`reqwest`) к LM Studio: `POST http://localhost:1234/v1/chat/completions`.
- OpenAI-совместимый формат.
- Режимы: `off`, `clean`, `format`, `command`.
- Редактируемый системный промт для режима `clean`.
- При ошибке LLM — fallback на сырой транскрипт.

### `app_commands/` — голосовые команды
- Переключение на уже запущенные окна Windows (`EnumWindows` + fuzzy match заголовка).
- Запуск приложений из настроенного списка (`launch_apps`).
- Вызывается отдельным global shortcut (`command_hotkey`, default `Ctrl+Shift+Space`).

### `injection/` — вставка текста
- **SendInput** с `KEYEVENTF_UNICODE` — быстро, поддерживает русский/эмодзи/CJK.
- **Clipboard** — копирует текст, эмулирует `Ctrl+V`, восстанавливает старый буфер.
- Режим выбирается в настройках (`injection_mode`).
- **UIPI caveat:** injection блокируется в elevated-окнах.

### Глобальная горячая клавиша
- `tauri-plugin-global-shortcut`.
- Дефолт: `Ctrl+Space` — push-to-talk (зажать и говорить).
- Настраивается в Settings; новый hotkey применяется сразу после сохранения.

### `pipeline/` — оркестратор
- FSM и события.
- Запускает/останавливает запись, дёргает `stt`, `llm`, `injection`.

### `application/`, `ipc/` и `fono-core`
- `ipc/*` содержит только Tauri command facade и сохраняет публичные имена IPC.
- `application/*` владеет orchestration dictation, diagnostics, wake sample и
  cancellable model downloads.
- `crates/fono-core` содержит независимые от Tauri operation FSM, coordinator и
  cancellation policy.
- Settings/history используют versioned atomic documents с backup и migration;
  LLM API key хранится в Windows Credential Manager и не возвращается renderer'у.
- Standalone STT workers используют protocol v2: handshake/capabilities,
  request/operation IDs, frame limits, health и graceful shutdown.

## 4. Структура каталогов

```
fono/
├── README.md
├── docs/
│   ├── architecture.md
│   ├── development.md
│   ├── roadmap.md
│   └── testing.md
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── application/
│       ├── ipc/
│       ├── events.rs
│       ├── history.rs
│       ├── secrets.rs
│       ├── state.rs
│       ├── error.rs
│       ├── audio/
│       ├── wakeword/
│       ├── vad/
│       ├── stt/
│       ├── llm/
│       ├── injection/
│       └── pipeline/
├── src/                         ← Frontend (React + Vite + TS)
│   ├── main.tsx
│   ├── App.tsx
│   ├── views/
│   │   ├── Onboarding.tsx
│   │   ├── Settings.tsx
│   │   └── Overlay.tsx
│   ├── components/
│   ├── lib/
│   │   ├── ipc.ts
│   │   └── types.ts
│   └── styles/
├── package.json
├── tsconfig.json
├── vite.config.ts
└── index.html
```

## 5. Потоки данных и потокобезопасность

- **Аудио owner thread** владеет cpal stream и пишет в bounded recording buffer.
- **Wake word поток**: читает буфер, прогоняет модель, при срабатывании будит pipeline.
- **Pipeline**: `fono-core::OperationCoordinator`, async waits и
  `spawn_blocking` для CPU/native STT.
- **UI**: Tauri webview, получает состояние через события.
- Глобальное состояние в `tauri::State<AppState>`.

## 6. Производительность

| Стадия | Реальная задержка |
|---|---|
| Wake word → начало записи | ~1.5 сек (частота чанков) |
| End-of-speech → старт STT | < 100 мс (VAD) |
| STT (short phrase, base, CUDA) | ~0.3–1 сек |
| STT (short phrase, base, CPU) | ~0.5–2 сек |
| LLM (Qwen2.5-7B Q4, CPU) | 0.5–3 сек |
| Injection (SendInput) | < 50 мс |
| **End-to-end** | **1–4 секунды** |

## 7. Безопасность и приватность

- Аудио не пишется на диск (кроме отладочных логов).
- Транскрипты не отправляются наружу, кроме явного вызова LLM.
- LM Studio можно заменить на любой OpenAI-совместимый эндпоинт.

## 8. Известные ограничения

| Ограничение | Пояснение |
|---|---|
| Elevated-окна | `SendInput` блокируется UIPI в окнах, запущенных от администратора |
| Смена hotkey | Применяется сразу после сохранения настроек |
| Wake word фраза | Меняется только когда wake word выключен |
| Голосовые команды | Базовое распознавание явного префикса; command hotkey и wake-команда требуют preview → confirm, без LLM-интерпретации |
| Автозапуск Windows | Сознательно не реализован |
| VAD-автостоп push-to-talk | Сознательно не реализован — управление только через клавишу |
| Telegram / защищённые приложения | Используйте режим «Буфер обмена» в настройках |
