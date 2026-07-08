# Разработка WhisperClone



Инструкция по подготовке окружения, сборке, дебагу и добавлению новых модулей.



## 1. Требования к окружению



| Инструмент | Версия | Зачем |

|---|---|---|

| [Rust](https://rustup.rs/) | >= 1.75 | Ядро на Rust + Tauri backend |

| [Node.js](https://nodejs.org/) | >= 20 (LTS) | Frontend, dev-сервер |

| npm | >= 10 | Менеджер пакетов (можно pnpm/yarn) |

| [MSVC Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2022 | Компиляция Rust (MSVC target) и whisper.cpp |

| Windows SDK | 10+ | Win32 API (`SendInput`, hooks) |

| [LM Studio](https://lmstudio.ai/) | latest | Локальный LLM-сервер (опционально) |

| [CMake](https://cmake.org/) | >= 3.18 | Сборка whisper.cpp |

| [LLVM / libclang](https://llvm.org/) | latest | bindgen для whisper-rs |

| CUDA Toolkit (опц.) | 12.x+ | GPU-ускорение whisper (CUDA) |

| Vulkan SDK (опц.) | latest | GPU-ускорение whisper (Vulkan) |



### Проверка



```bash

rustc --version    # >= 1.75

cargo --version

node --version     # >= 20

npm --version

```



### MSVC toolchain



Rust на Windows по умолчанию использует `x86_64-pc-windows-msvc`. Убедитесь, что

установлены «Desktop development with C++» workload в Visual Studio Installer,

включая **Windows 10/11 SDK**. Это нужно и для Tauri, и для сборки whisper.cpp.



Альтернатива — `x86_64-pc-windows-gnu` (MinGW), но **MSVC рекомендуется** и лучше

поддерживается экосистемой.



## 2. Установка зависимостей



```bash

# Из корня репозитория

npm install

```



Tauri CLI идёт как dev-зависимость (`@tauri-apps/cli`) — отдельная установка не нужна.



Rust-зависимости подтянутся автоматически при первом `cargo build` / `npm run tauri dev`.
При сборке с CUDA/Vulkan убедитесь, что `LIBCLANG_PATH` указывает на `LLVM/bin`, а в PATH есть
CMake, CUDA/Vulkan и MSVC.

При сборке с CUDA/Vulkan убедитесь, что `LIBCLANG_PATH` указывает на `LLVM/bin`, а в PATH есть

CMake, CUDA/Vulkan и MSVC.



### ⚠️ Критичное правило сборки



**ВСЕГДА собирайте production-бинарь только через `tauri build`, никогда — через `cargo build --release` напрямую.**



```bash

# ✅ ПРАВИЛЬНО — tauri build встраивает фронтенд в exe

npm run tauri build -- --no-bundle



# ❌ НЕПРАВИЛЬНО — даст неработающий exe с ошибкой "localhost:1420 отказано в подключении"

cargo build --release

```



**Почему:** `tauri build` запускает `beforeBuildCommand` (т.е. `npm run build`, собирающий `dist/`) и через `tauri-build` в `build.rs` встраивает `dist/` прямо в exe как ресурс. `cargo build --release` пропускает оба шага — exe остаётся с устаревшим (или пустым) указателем на фронтенд и пытается грузить его с dev-сервера `localhost:1420`, которого нет.



**`cargo check` / `cargo build` (debug, без `--release`) — можно использовать** для быстрой проверки компиляции Rust-кода; они не плодят distributable exe.



### Когда exe занят и не пересобирается



Если при пересборке получаете `error: failed to remove file whisperclone.exe ... Отказано в доступе (os error 5)` — приложение запущено. Убейте процесс и повторите:



```bash

cmd //c "taskkill /F /IM whisperclone.exe"

```



## 3. Запуск в dev-режиме



```bash

npm run tauri dev

```



Это запустит:

1. Vite dev-сервер для фронтенда (по умолчанию `http://localhost:1420`).

2. Компиляцию Rust-ядра (`cargo build`).

3. Открытие окна приложения с горячей перезагрузкой фронтенда.



Первая сборка может занять **5–15 минут** (тянутся и компилируются `cpal`, `whisper-rs`,

`windows` с тонной Win32 binding'ов). Последующие — быстро благодаря инкрементальной компиляции.



### Дебаг Rust-кода



- В VS Code / RustRover: launch configuration, подключаемая к процессу `tauri dev`.

- Логи: используем крейт [`tracing`](https://crates.io/crates/tracing) +

  `tracing-subscriber` с уровнем `debug` в dev-сборке.

- Для проверки IPC-команд удобно дёргать их напрямую из devtools фронтенда

  (`F12` в окне приложения → Console).



## 4. Сборка whisper.cpp



[`whisper-rs`](https://crates.io/crates/whisper-rs) автоматически собирает whisper.cpp

через build-script при первом `cargo build` — отдельная установка не требуется.



### GPU-ускорение



По умолчанию `Cargo.toml` включает feature `cuda`:



```toml

[features]

default = ["cuda"]

cuda = ["whisper-rs/cuda"]

```



Для сборки на машине без NVIDIA GPU используйте:



```bash

npm run tauri build -- --no-bundle --no-default-features

```



Также доступна feature `vulkan` для GPU от AMD/Intel/NVIDIA через Vulkan:



```bash

npm run tauri build -- --no-bundle --no-default-features --features vulkan

```



Требования:

- **CUDA**: CUDA Toolkit + MSVC + CMake + LLVM/libclang.

- **Vulkan**: Vulkan SDK + MSVC + CMake + LLVM/libclang.



Runtime-переключатель `use_gpu` в настройках позволяет отключить GPU без пересборки.



## 5. Whisper-модели



Модели **не коммитятся** в репозиторий (большой размер). Скачивайте через UI приложения

или вручную в каталог:



```

%APPDATA%\WhisperClone\whisper-models\

```



Пример ручной загрузки base-модели:



```bash

mkdir -p "$APPDATA/WhisperClone/whisper-models"

curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin \

  -o "$APPDATA/WhisperClone/whisper-models/ggml-base.bin"

```



| Модель

|---|---|---|---|

| `tiny` | 75 МБ | Очень быстро | Низкая |

| `base` | 140 МБ | Быстро | Приемлемая (RU OK) |

| `small` | 460 МБ | Средне | Хорошая |

| `medium` | 1.5 ГБ | Медленно | Очень хорошая |

| `large-v3` | 3 ГБ | Медленно | SOTA |



**Рекомендация для CPU:** `base` или `small`. Для GPU — `medium`/`large`.



## 6. LM Studio



1. Установите [LM Studio](https://lmstudio.ai/).

2. Загрузите модель (рекомендация: Qwen2.5-12B-Instruct или Llama-3-8B-Instruct,

   желательно в Q4_K_M квантовании — быстрее на CPU).

3. Включите **Local Server** на порту `1234` (default).

4. Проверьте: `curl http://localhost:1234/v1/models`.



Приложение стучится на `http://localhost:1234/v1/chat/completions` — формат запросов

совместим с OpenAI API.



## 7. Сборка инсталлятора (Этап 7)



```bash

npm run tauri build

```



Результаты появятся в `src-tauri/target/release/bundle/`:

- `msi/WhisperClone_0.1.0_x64.msi` — MSI-инсталлер.

- `nsis/WhisperClone_0.1.0_x64-setup.exe` — NSIS-инсталлер.



### Подпись кода (рекомендуется)



Без подписи Windows SmartScreen будет пугать пользователей. Для подписи нужен

**code signing certificate** (EV или OV). Команда:



```bash

npm run tauri build -- --signing-identity <THUMBPRINT>

```



См. [Tauri docs: Code Signing](https://v2.tauri.app/distribute/sign-windows-applications/).



### Автообновление



Подключается через `tauri-plugin-updater` + JSON-манифест версий на GitHub Releases.

См. [Updater Plugin](https://v2.tauri.app/plugin/updater/).



## 8. Структура IPC-команд



Tauri IPC команды объявляются в `src-tauri/src/commands.rs` и регистрируются в

`tauri::Builder::invoke_handler`. На фронтенде они вызываются через `@tauri-apps/api`:



```typescript

import { invoke } from '@tauri-apps/api/core';



const devices = await invoke<DeviceInfo[]>('list_audio_devices');

const transcript = await invoke<string>('transcribe_clip', { durationMs: 3000 });

```



Подробный список команд — в `docs/architecture.md` → раздел `commands.rs`.



## 9. Линтинг и форматирование



```bash

# Rust

cargo fmt

cargo clippy --all-targets -- -D warnings



# Frontend

npm run build     # typecheck + vite build

npm run format    # prettier

```



> `npm run lint` временно не работает из-за отсутствующего пакета `typescript-eslint`

> в `eslint.config.js`.



## 10. Отладка аудио



Если не захватывается звук:

1. Проверьте устройство по умолчанию в Windows → Settings → Sound.

2. Запустите пример `cpal` `beep` напрямую — убедитесь, что крейт работает.

3. Включите `RUST_LOG=whisperclone::audio=debug` и смотрите логи чанков.

4. При необходимости включите `RUST_LOG=debug` и проверьте логи:

   `%APPDATA%\WhisperClone\logs\whisperclone.log.YYYY-MM-DD`.



## 11. Добавление нового модуля



1. Создайте каталог `src-tauri/src/<module>/` с `mod.rs`.

2. Объявите модуль в `src-tauri/src/lib.rs` (`pub mod <module>;`).

3. Если нужны IPC-команды — добавьте в `commands.rs` и зарегистрируйте в `main.rs`.

4. Если нужна UI-часть — создайте компонент в `src/components/` и подключите в нужной view.

5. Опишите публичный API модуля комментарием в `mod.rs` и при необходимости — в `docs/architecture.md`.



## 12. Известные проблемы платформы



- **UIPI / elevated windows:** `SendInput` не работает в окна, запущенные с правами

  администратора, если само приложение не повышено. Решение — опция «запуск от администратора».

- **DirectInput / raw input игры:** некоторые игры игнорируют `SendInput`. Это известное

  ограничение; используется только в текстовых полях.

- **Антивирусы:** поведение, похожее на кейлоггер (глобальные хуки), иногда триггерит AV.

  Решение — подпись кода + явные объяснения в EULA.

