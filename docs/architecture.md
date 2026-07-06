# Архитектура WhisperClone

Этот документ описывает модульную структуру приложения, потоки данных и
ключевые технические решения.

## 1. Высокоуровневая схема

```
┌──────────────────────────────────────────────────────────────────┐
│                       Tauri 2 Application                        │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  Frontend (Webview, React + TypeScript + Tailwind)         │  │
│  │  - Onboarding, Settings, Overlay, Tray menu               │  │
│  │  - IPC через tauri::invoke                                │  │
│  └─────────────────────────────┬──────────────────────────────┘  │
│                                │  invoke / events                │
│  ┌─────────────────────────────┴──────────────────────────────┐  │
│  │                     Rust Core (главный процесс)            │  │
│  │                                                            │  │
│  │  ┌─────────────┐    ┌──────────────┐   ┌──────────────┐    │  │
│  │  │   audio/    │───▶│   wakeword/  │   │   hotkey/    │    │  │
│  │  │  cpal, ring │    │ always-on KW │   │ globalshortcut│   │  │
│  │  └─────┬───────┘    └──────┬───────┘   └──────┬───────┘    │  │
│  │        │                   │                  │            │  │
│  │        ▼     ┌─────────────────────────┐      │            │  │
│  │      ┌──────────────┐                  │      │            │  │
│  │      │    vad/      │     pipeline/ (оркестратор) ◀────────┘  │  │
│  │      │ silero-vad   │                  │                     │  │
│  │      └─────┬────────┘                  │                     │  │
│  │            ▼                           ▼                     │  │
│  │      ┌──────────────┐            ┌──────────────┐            │  │
│  │      │    stt/      │───────────▶│    llm/      │            │  │
│  │      │ whisper.cpp  │            │ LM Studio HTTP│           │  │
│  │      └──────────────┘            └──────┬───────┘            │  │
│  │                                         ▼                    │  │
│  │                                  ┌──────────────┐            │  │
│  │                                  │  injection/  │            │  │
│  │                                  │  SendInput   │            │  │
│  │                                  └──────────────┘            │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

## 2. Основной конвейер (Pipeline)

Конвейер — это конечный автомат с состояниями:

```
        ┌──────────┐
        │  IDLE    │  ◀── всегда слушает wake word (низкий CPU)
        └────┬─────┘
   wake word  │   или глобальная горячая клавиша
   обнаружена │
             ▼
        ┌──────────┐
        │ LISTENING│  ◀── запись аудио в ring buffer + VAD
        └────┬─────┘
   VAD: конец речи (тишина N мс) или повторная wake-фраза
             │
             ▼
        ┌──────────┐
        │TRANSCRIB-│  ◀── whisper.cpp декодит накопленный буфер
        │  ING     │       (или streaming с partial results)
        └────┬─────┘
             ▼
        ┌──────────┐
        │PROCESSING│  ◀── (опционально) LLM-постобработка
        └────┬─────┘
             ▼
        ┌──────────┐
        │ INJECTING│  ◀── SendInput: вставка текста в активное окно
        └────┬─────┘
             │
             ▼
        ┌──────────┐
        │   IDLE   │
        └──────────┘
```

### События для UI

Каждая смена состояния эмитит Tauri-событие (`state-change`), которое фронтенд
слушает и обновляет overlay-индикатор:
- `idle` → спрятать overlay (или показать мини-иконку в трее).
- `listening` → показываем красный кружок «● слушаю».
- `transcribing` / `processing` → анимация «думаю…».
- `injecting` → короткий «вставляю…».
- `error` → красная иконка с сообщением.

## 3. Описание модулей

### `audio/` — захват аудио
- Использует крейт `cpal` (WASAPI на Windows).
- Захватывает дефолтный микрофон (можно выбрать в настройках) на **16 кГц, моно, i16** —
  формат, ожидаемый whisper.cpp.
- Записывает сэмплы в **lock-free ring buffer** (SPSC), откуда их читают
  модули wake word, VAD и STT.
- Отдельный поток (`std::thread` или `tokio::task::spawn_blocking`) — аудио не должно блокировать UI.

**Ключевые типы:**
```rust
pub struct AudioCapture { /* cpal::Stream */ }
impl AudioCapture {
    pub fn start(device: Option<DeviceId>, on_samples: Box<dyn Fn(&[i16])>) -> Result<()>;
    pub fn stop(&self);
    pub fn list_devices() -> Vec<DeviceInfo>;
}
```

### `wakeword/` — детекция ключевой фразы
- Запускается **только** в состоянии `IDLE` (когда диктовка не идёт).
- Непрерывно читает чанки из аудио-ring buffer и прогоняет через модель wake word.
- На срабатывании — посылает событие в pipeline.
- Лёгкий CPU: целевой расход 1–3%.

**Альтернативы (выбрать на Этапе 4):**
- **openWakeWord** (MIT, English-only) — запускать как Python sidecar через
  `tauri-plugin-python` или перекомпилировать в ONNX и прогонять через `ort` (ONNX Runtime в Rust).
- **Porcupine** (Picovoice) — кросс-язычный, есть Rust binding, бесплатный tier с ограничениями,
  обучение кастомной русской фразы через Picovoice Console.
- **Своя ONNX-модель**, обученная через [livekit-wakeword](https://livekit.com/blog/livekit-wakeword) —
  полностью своя, MIT.

### `vad/` — Voice Activity Detection
- Определяет границы речи: где началась и где закончилась.
- Используется для:
  - Запуска STT при обнаружении речи (экономит CPU).
  - Определения конца фразы (N мс тишины) для автоматической остановки диктовки.
- **Рекомендация:** silero-vad (быстрая, точная, доступна как ONNX → крейт `ort`).

### `stt/` — Speech-to-Text
- Биндинги к [whisper.cpp](https://github.com/ggerganov/whisper.cpp) через
  [`whisper-rs`](https://crates.io/crates/whisper-rs) (идиоматическая обёртка над C API).
- Принимает `&[i16]` PCM 16 кГц, возвращает транскрибированный текст + тайминги.
- Поддержка моделей: `tiny`, `base`, `small`, `medium`, `large` (GGML).
- GPU-ускорение: опциональная сборка с CUDA / DirectML для большой скорости.

```rust
pub struct WhisperModel { /* whisper_rs::WhisperContext */ }
impl WhisperModel {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn transcribe(&self, samples: &[i16], lang: Language) -> Result<Transcript>;
}
pub struct Transcript { pub text: String, pub segments: Vec<Segment> }
```

### `llm/` — AI-постобработка
- HTTP-клиент (`reqwest`) к LM Studio: `POST http://localhost:1234/v1/chat/completions`.
- OpenAI-совместимый формат запросов/ответов.
- **Режимы обработки** (выбираются в настройках или командой):
  - `off` — выключено, выдаём «сырой» транскрипт.
  - `clean` (по умолчанию) — убрать «ээ/мм», добавить пунктуацию, исправить явные оговорки.
  - `format` — оформить в абзацы / списки.
  - `command` — команды через префикс («команда: переведи на английский»).
- Стриминг ответа (SSE) для мгновенной обратной связи.
- Промпт-шаблоны с жёстким системным сообщением, требующим сохранить смысл.

### `injection/` — вставка текста
- Использует крейт [`windows`](https://crates.io/crates/windows) → `SendInput` с `KEYEVENTF_UNICODE`.
- `SendInput` с Unicode-символами работает с русским, эмодзи, CJK.
- **Альтернативный режим для длинных текстов** — вставка через буфер обмена
  (`OpenClipboard`/`SetClipboardData`) + симуляция `Ctrl+V` (с восстановлением
  предыдущего содержимого буфера).
- **UIPI caveat:** injection блокируется в elevated-окнах (запущенных от администратора).
  Документируем как ограничение; опция «запускать приложение от администратора».

### `hotkey/` — глобальные горячие клавиши
- `tauri-plugin-global-shortcut` (Tauri v2).
- Дефолт: `Ctrl+Space` — push-to-talk (зажать и говорить).
- Настраивается в Settings.

### `pipeline/` — оркестратор
- Держит текущее состояние конвейера (`Mutex<PipelineState>`).
- Подписан на события от `wakeword`, `vad`, `hotkey`.
- Запускает/останавливает запись, дёргает `stt` и `llm`, затем `injection`.
- Эмитит события смены состояния для UI.

### `commands.rs` — Tauri IPC
- Инвокабельные из фронтенда команды:
  - `get_state() -> PipelineState`
  - `start_dictation()` / `stop_dictation()`
  - `list_audio_devices() -> Vec<DeviceInfo>`
  - `list_whisper_models() -> Vec<ModelInfo>`
  - `test_llm_connection() -> Result<String>`
  - `get_settings() -> Settings` / `save_settings(Settings)`
  - `set_hotkey(String)`, `set_wake_word(String)`

## 4. Структура каталогов

```
whisperclone/
├── README.md
├── docs/
│   ├── architecture.md          ← этот файл
│   ├── development.md
│   └── roadmap.md
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── icons/
│   ├── resources/
│   │   ├── whisper/             ← *.bin модели (gitignored, скачиваются)
│   │   └── wakeword/            ← *.onnx / *.ppn модели
│   └── src/
│       ├── main.rs              ← точка входа, tauri::Builder, plugins
│       ├── lib.rs               ← реэкспорт модулей
│       ├── commands.rs          ← Tauri IPC команды
│       ├── state.rs             ← AppState (Arc<Mutex<...>>)
│       ├── error.rs             ← типы ошибок (thiserror)
│       ├── audio/
│       │   ├── mod.rs
│       │   └── capture.rs
│       ├── wakeword/
│       │   └── mod.rs
│       ├── vad/
│       │   └── mod.rs
│       ├── stt/
│       │   └── mod.rs
│       ├── llm/
│       │   ├── mod.rs
│       │   └── prompts.rs
│       ├── injection/
│       │   └── mod.rs
│       ├── hotkey/
│       │   └── mod.rs
│       └── pipeline/
│           ├── mod.rs
│           └── state.rs
├── src/                         ← Frontend (React + Vite + TS)
│   ├── main.tsx
│   ├── App.tsx
│   ├── views/
│   │   ├── Onboarding.tsx
│   │   ├── Settings.tsx
│   │   └── Overlay.tsx
│   ├── components/
│   │   ├── StatusBadge.tsx
│   │   ├── MicSelector.tsx
│   │   └── ModelManager.tsx
│   ├── lib/
│   │   ├── ipc.ts               ← типизированный invoke-клиент
│   │   └── types.ts             ← shared типы (PipelineState, Settings…)
│   └── styles/
│       └── globals.css          ← Tailwind layers
├── package.json
├── tsconfig.json
├── vite.config.ts
└── index.html
```

## 5. Потоки данных и потокобезопасность

- **Аудио-поток** (real-time поток cpal): пишет в lock-free SPSC queue.
- **Wake word поток**: читает очередь, прогоняет модель, при срабатывании будит pipeline.
- **Pipeline поток** (tokio task): владеет FSM,协调 запись, VAD, STT, LLM, injection.
- **UI поток** (Tauri main): только рисует состояние, не блокирует.
- Состояние (state) — `Arc<Mutex<AppState>>` или `Arc<RwLock<…>>` в `tauri::State`.

## 6. Производительность и-latency цели

| Стадия | Целевая задержка |
|---|---|
| Wake word → начало записи | < 200 мс |
| End-of-speech → старт STT | < 100 мс (VAD) |
| STT (short phrase, base model, CPU) | 300–800 мс |
| LLM (LM Studio, 12B Q4, 1 предложение) | 500–2000 мс |
| Injection (SendInput) | < 50 мс |
| **End-to-end** | **1–3 секунды** |

Оптимизации (Этап 8):
- GPU-ускорение whisper.cpp (CUDA/DirectML).
- Streaming-транскрипция с partial results.
- Квантование LLM и/или smaller model для `clean` режима.

## 7. Безопасность и приватность

- Аудио **не пишется на диск** (если пользователь явно не включит отладку).
- Транскрипты не отправляются наружу, кроме явного вызова LLM.
- LM Studio можно заменить на любой OpenAI-совместимый эндпоинт
  (включая облако) — это явная опция пользователя.

## 8. Известные ограничения и риски

| Риск / ограничение | Решение / митигация |
|---|---|
| Wake word для русского — нет готовой open-source модели | Обучить свою (livekit-wakeword) или взять Porcupine с кастомной фразой |
| always-on аудио → расход CPU | Wake word на лёгком потоке + VAD-gating; спим между чанками |
| `SendInput` блокируется UIPI в elevated-окнах | Документируем; опциональный запуск от администратора |
| Whisper.cpp медленно на CPU | GPU-билд; streaming; smaller модель для быстрого отклика |
| LM Studio 12B медленно | Уменьшить модель; квантование; fallback на облако |
