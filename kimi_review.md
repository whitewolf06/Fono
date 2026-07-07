# WhisperClone — обзор и анализ проекта

> Дата анализа: 2026-07-07  
> Ветка: `main`  
> Состояние рабочей копии: 3 незакоммиченных изменения (`src-tauri/src/commands.rs`, `src/lib/ipc.ts`, `src/views/Settings.tsx`).

---

## 1. Назначение проекта

**WhisperClone** — десктопное приложение для Windows, которое превращает речь в текст и вставляет результат в активное окно ввода. Позиционируется как локальный аналог Wispr Flow: всё распознавание и постобработка происходят на компьютере пользователя.

### Ключевые возможности

- **Активация по ключевой фразе** (wake word) — пока только UI-опция, сам детектор не реализован.
- **Push-to-talk** — глобальная горячая клавиша `Ctrl+Space` (по умолчанию), но на текущем этапе регистрация глобального хоткея отключена.
- **Локальная транскрипция** через `whisper.cpp` (крейт `whisper-rs`) с моделями `tiny/base/small/medium/large`.
- **AI-постобработка** через локальный сервер LM Studio (`http://localhost:1234/v1/chat/completions`) — режимы `off/clean/format/command`.
- **Вставка текста в активное окно** через Win32 `SendInput` (`KEYEVENTF_UNICODE`) — поддержка русского, эмодзи и CJK.
- **System tray** с пунктами «Открыть настройки», «Пауза/Resume», «Выход».
- **Overlay-индикатор** статуса конвейера (слушаю / распознаю / вставляю).
- **Менеджер whisper-моделей** со скачиванием из Hugging Face прямо в UI.
- **Тест микрофона** с отображением RMS/peak.

---

## 2. Технологический стек

| Слой | Технологии |
|---|---|
| Каркас приложения | Tauri v2.1 (`src-tauri/`) |
| Фронтенд | React 18.3 + TypeScript 5.6 + Vite 6 + Tailwind CSS 3.4 |
| Сборка фронтенда | `vite build` → `dist/`, встраивается в exe Tauri |
| Бэкенд / системное | Rust 1.75+, `cpal` (WASAPI), `whisper-rs` 0.16, `windows` 0.58, `arboard`, `reqwest` |
| Асинхронность / состояние | `tokio`, `parking_lot::Mutex`, `tauri::State` |
| Сериализация | `serde`/`serde_json` |
| Логирование | `tracing` + `tracing-subscriber` + `tracing-appender` (файл + консоль) |
| Линтинг / форматирование | ESLint 9 (`typescript-eslint`), Prettier 3, `cargo fmt`/`clippy` |

---

## 3. Архитектура и структура директорий

```
whisperclone/
├── docs/                       # документация
│   ├── architecture.md
│   ├── development.md
│   ├── roadmap.md
│   └── testing.md
├── src/                        # фронтенд (React + TS)
│   ├── main.tsx                # точка входа, выбор view по ?view=
│   ├── App.tsx                 # маршрутизация settings/overlay/onboarding
│   ├── lib/
│   │   ├── ipc.ts              # типизированный invoke-клиент
│   │   └── types.ts            # shared типы
│   ├── views/
│   │   ├── Settings.tsx        # основное окно настроек
│   │   ├── Overlay.tsx         # оверлей-индикатор
│   │   └── Onboarding.tsx      # wizard онбординга
│   ├── components/
│   │   ├── MicSelector.tsx     # выбор микрофона
│   │   ├── MicTest.tsx         # тест уровня звука
│   │   ├── ModelManager.tsx    # скачивание/выбор whisper-модели
│   │   └── StatusBadge.tsx     # бейдж состояния
│   └── styles/globals.css      # Tailwind + компонентные классы
├── src-tauri/                  # Rust + Tauri
│   ├── Cargo.toml
│   ├── tauri.conf.json         # окна, иконки, bundle, capabilities
│   ├── build.rs                # tauri_build::build()
│   ├── capabilities/default.json
│   ├── icons/                  # иконки приложения
│   ├── resources/whisper/      # *.bin модели (gitignored)
│   └── src/
│       ├── main.rs             # whisperclone_lib::run()
│       ├── lib.rs              # run(), инициализация плагинов/трея/фоновой задачи
│       ├── commands.rs         # Tauri IPC команды
│       ├── state.rs            # AppState + пути к данным/настройкам
│       ├── error.rs            # AppError + AppResult
│       ├── types.rs            # зеркальные Rust-типы для TS
│       ├── audio/mod.rs        # захват аудио через cpal
│       ├── stt/mod.rs          # whisper.cpp STT
│       ├── llm/mod.rs          # HTTP-клиент к LM Studio
│       ├── injection/mod.rs    # SendInput
│       └── pipeline/mod.rs     # FSM, запись/остановка, оркестрация
```

> **Важно:** в `docs/architecture.md` описаны модули `wakeword/`, `vad/`, `hotkey/`, но в `src-tauri/src/` их нет — они ещё не реализованы.

---

## 4. Основные компоненты и их взаимодействие

### Фронтенд

- **`src/main.tsx`** выбирает view по query-параметру `?view=`.
- **`src/App.tsx`** рендерит `SettingsView`, `OverlayView` или `OnboardingView`.
- **`src/lib/ipc.ts`** — централизованный IPC-клиент. Основные методы:
  - `getPipelineState`, `startDictation`, `stopDictation`
  - `transcribeTest`, `listAudioDevices`, `listWhisperModels`
  - `downloadWhisperModel`, `testLlmConnection`
  - `getSettings`, `saveSettings`, `getRecentLogs`, `testMicrophone`
- **`src/views/Settings.tsx`** — основное окно с секциями: микрофон, модель, активация, AI-обработка, тест транскрипции, прочее.

### Бэкенд

- **`src-tauri/src/lib.rs`**
  - Инициализирует `tracing` (логи в `%APPDATA%\WhisperClone\logs\whisperclone.log`).
  - Регистрирует плагины: `shell`, `opener`, `dialog`, `fs`, `global-shortcut`.
  - Создаёт `AppState` и `Pipeline`, добавляет их в `tauri::State`.
  - Настраивает system tray.
  - Запускает фоновую задачу `pipeline::start_background`.
  - Глобальный хоткей push-to-talk **временно отключён**:
    ```rust
    // NOTE: push-to-talk (global shortcut) временно отключён до Этапа 3.
    // setup_global_shortcut(app)?;
    ```

- **`src-tauri/src/pipeline/mod.rs`** — сердце конвейера:
  - `start_recording(device_id)` — запускает `AudioCapture::start`, пишет сэмплы в разделяемый буфер.
  - `stop_recording()` — останавливает поток `cpal` и возвращает накопленные сэмплы.
  - `set_state(...)` — обновляет `PipelineState`, эмитит событие `pipeline-state`, показывает/скрывает overlay.

- **`src-tauri/src/audio/mod.rs`** — обёртка над `cpal`:
  - Список микрофонов.
  - Открытие потока, конвертация любого `SampleFormat` в `i16`, микширование в mono, ресемплинг до 16 кГц.

- **`src-tauri/src/stt/mod.rs`** — `whisper-rs`:
  - Ленивая загрузка GGML-модели.
  - Синхронный вызов `whisper.cpp` внутри `spawn_blocking`.
  - Вывод whisper в stdout/stderr отключён, чтобы не зависал Tauri webview.

- **`src-tauri/src/llm/mod.rs`** — `LlmClient`:
  - `test_connection()` — GET `/models`.
  - `process(text, mode)` — POST `/chat/completions` с системными промптами.

- **`src-tauri/src/injection/mod.rs`** — вставка текста через `SendInput` пачками по 16 символов с задержкой 2 мс.

### Поток данных

```
UI (Settings/Overlay)  ← invoke / events →  Rust Core
                                       │
                    ┌──────────────────┼──────────────────┐
                    ▼                  ▼                  ▼
              audio::start      pipeline::start      stt::transcribe
                    │             recording              │
                    │                  │                 ▼
                    └────────►  Vec<i16>  ─────────►  whisper.cpp
                                                       │
                                              llm::process (opt)
                                                       │
                                               injection::inject_text
                                                       │
                                              SendInput → активное окно
```

---

## 5. Конфигурация

### `src-tauri/tauri.conf.json`

- Два окна:
  - `settings` — 900×720, обычное декорированное окно.
  - `overlay` — 280×56, `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`, скрыто по умолчанию.
- Tray: иконка `icons/icon.png`, меню по клику.
- Bundle: MSI + NSIS, категория `Productivity`.
- `resources/whisper/*` включаются в дистрибутив.
- CSP отключён (`"csp": null`).

### `src-tauri/capabilities/default.json`

Разрешения для окон `settings` и `overlay`: `core:default`, `core:event:default`, `core:window:*`, `shell:allow-open`, `opener:default`, `fs:allow-read-text-file`, `fs:allow-write-text-file`, `global-shortcut:allow-register`, `global-shortcut:allow-unregister`.

### `src-tauri/Cargo.toml`

- `tauri` с feature `tray-icon`.
- `whisper-rs = "0.16"` (CPU-only; GPU-фича `cuda` закомментирована).
- `windows` 0.58 с features для `SendInput`.
- Release-профиль: `lto = true`, `opt-level = "s"`, `panic = "abort"`, `strip = true`.

---

## 6. Скрипты и зависимости

### `package.json`

```json
{
  "name": "whisperclone",
  "version": "0.1.0",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "lint": "eslint . --ext ts,tsx --report-unused-disable-directives --max-warnings 0",
    "format": "prettier --write \"src/**/*.{ts,tsx,css}\""
  },
  "dependencies": {
    "@tauri-apps/api": "^2.1.1",
    "@tauri-apps/plugin-global-shortcut": "^2.0.0",
    "@tauri-apps/plugin-shell": "^2.0.1",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.1.0",
    "@vitejs/plugin-react": "^4.3.4",
    "tailwindcss": "^3.4.15",
    "typescript": "^5.6.3",
    "vite": "^6.0.1",
    "eslint": "^9.15.0",
    "prettier": "^3.4.1"
  }
}
```

### `src-tauri/Cargo.toml`

- Tauri: `tauri ^2.1`, `tauri-plugin-shell ^2.0`, `tauri-plugin-global-shortcut ^2.0`, `tauri-plugin-dialog ^2.0`, `tauri-plugin-fs ^2.0`, `tauri-plugin-opener ^2.0`.
- Аудио: `cpal ^0.15`.
- STT: `whisper-rs ^0.16`.
- Win32: `windows ^0.58`, `arboard ^3.4`.
- HTTP: `reqwest ^0.12`.
- Async/утилиты: `tokio ^1.41`, `anyhow`, `thiserror ^2.0`, `parking_lot`, `crossbeam-queue`, `crossbeam-channel`, `once_cell`, `dirs`, `chrono`.
- Логи: `tracing`, `tracing-subscriber`, `tracing-appender`.

---

## 7. Тесты

### Автоматизированные тесты

- **Rust:** единственные unit-тесты находятся в `src-tauri/src/audio/mod.rs` под `#[cfg(test)]`. Проверяют конвертацию сэмплов разных форматов (`i8/i16/i32/i64/u8/u16/u32/u64/f32/f64`) в `i16`.
- **Фронтенд:** тестов нет. В `package.json` нет скрипта `test`.
- **Интеграционных/E2E-тестов нет.**

### Ручное тестирование

`docs/testing.md` содержит чек-лист ручного QA:
1. UI открывается, трей работает.
2. Список микрофонов заполняется.
3. Скачивание whisper-модели (`base` ~147 МБ).
4. Подключение к LM Studio (опционально).
5. Сохранение настроек в `%APPDATA%\WhisperClone\settings.json`.
6. Тест транскрипции (с возможностью вставки в окно — в текущих незакоммиченных правках).

---

## 8. Найденные особенности, потенциальные проблемы и незавершённые части

### А. Незакоммиченные изменения (dirty files)

В трёх файлах добавлена опция `inject` для тестовой транскрипции:
- `src-tauri/src/commands.rs`: `transcribe_test` теперь принимает `inject: Option<bool>` и при `inject=true` вставляет результат в активное окно.
- `src/lib/ipc.ts`: сигнатура `transcribeTest` обновлена.
- `src/views/Settings.tsx`: добавлен чекбокс «Вставлять в активное окно», обновлён текст и предупреждение.

### Б. Отсутствующие модули

В `src-tauri/src/` **нет**:
- `vad/` — нет Voice Activity Detection, конец речи не определяется автоматически.
- `wakeword/` — нет детектора ключевой фразы.
- `hotkey/` — нет отдельного модуля глобальных горячих клавиш.

### В. Отключённый push-to-talk

В `src-tauri/src/lib.rs`:
```rust
// NOTE: push-to-talk (global shortcut) временно отключён до Этапа 3.
// setup_global_shortcut(app)?;
```

Также в `save_settings` намеренно не регистрируется хоткей.

### Г. Ручной режим диктовки

В текущей версии работает ручная диктовка через кнопки UI (`Start dictation` / `Stop and insert`) — запись, транскрипция, AI и вставка. Автоматической остановки по тишине нет.

### Д. Проблемы/недоделки LLM-обработки

- В UI **нет поля для выбора модели LM Studio** (`llm_model`). В `Settings` можно задать только URL.
- `LlmClient::process` требует, чтобы `llm_model` был `Some(...)`:
  ```rust
  let model = self.model.as_deref()
      .ok_or_else(|| AppError::Llm("модель LLM не указана в настройках".into()))?;
  ```
  При `ai_mode != off` и `llm_model == null` обработка упадёт с ошибкой.

### Е. Не реализованные фичи UI/UX

- **Автозапуск с Windows** — чекбокс сохраняется в настройках, но логики автозапуска нет.
- **Кастомная wake-фраза** — input заблокирован (`disabled`), в UI написано «доступно в следующей версии».
- **Вставка через буфер обмена** — TODO в `injection/mod.rs`:
  ```rust
  /// TODO (Этап 2): реализовать с восстановлением предыдущего буфера.
  pub fn inject_via_clipboard(_text: &str) -> AppResult<()> { ... }
  ```

### Ж. Конфигурация пакетного менеджера

- В репозитории одновременно присутствуют `package-lock.json` и `pnpm-lock.yaml`.
- `pnpm-workspace.yaml` содержит некорректный/незаполненный плейсхолдер:
  ```yaml
  allowBuilds:
    esbuild: set this to true or false
  ```
  Это может ломать `pnpm` (должен быть `packages:`). README предлагает `npm install`.

### И. Cargo.lock

`src-tauri/Cargo.lock` занесён в `.gitignore`. Для конечного бинарного приложения это плохо — сборки не воспроизводимы между разработчиками/CI.

### К. Неиспользуемые плагины

В `src-tauri/src/lib.rs` инициализируются `shell`, `opener`, `dialog`, `fs`, но в коде они не используются (настройки и модели читаются через `std::fs`, а не через Tauri FS API). Это лишние зависимости и capabilities.

### Л. Потенциальные низкоуровневые проблемы

- В `audio/mod.rs` используется `unsafe { bytes.align_to::<T>() }` для конвертации сэмплов — если буфер не выровнен, head/tail отбрасываются; для продакшена стоит использовать более безопасное копирование.
- `injection` пачками по 16 символов со `sleep(2 ms)` — при очень длинных текстах вставка будет медленной; планируется fallback через clipboard.

### М. Документация vs реальность

- `docs/roadmap.md` и `docs/testing.md` описывают Этапы 1–5 как «в разработке»; фактически базовая транскрипция и инъекция уже работают (MVP).
- `docs/testing.md` говорит, что push-to-talk, wake word, overlay и вставка текста «не работают» — часть из этого уже реализована в текущих dirty-файлах.

---

## 9. Итог

Проект находится в состоянии **раннего MVP**: базовый захват аудио, транскрипция whisper.cpp, вставка через `SendInput`, управление моделями и настройками уже работают. Основные недостающие части для полноценного продукта:

- **VAD** (автоостановка по тишине),
- **Wake word** (активация голосом),
- **Глобальный push-to-talk хоткей**,
- **Автозапуск с Windows**,
- **Выбор модели LLM в UI**,
- **Fallback-вставка через буфер обмена**,
- **Расширенное тестовое покрытие**.

Также стоит привести в порядок конфигурацию пакетного менеджера и включить `Cargo.lock` в репозиторий.
