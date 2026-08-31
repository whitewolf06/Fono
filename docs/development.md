# Разработка и release Fono

## Требования машины сборки

- Rust toolchain из `src-tauri/rust-toolchain.toml` и Node.js 20+.
- MSVC Build Tools 2022 + Windows SDK.
- CMake и LLVM/libclang.
- CUDA Toolkit для CUDA worker.
- Vulkan SDK для Vulkan worker.

CUDA Toolkit и Vulkan SDK нужны только разработчику, который создаёт release.
Пользователь получает CUDA runtime DLL рядом с CUDA worker; для Vulkan нужен
драйвер GPU с Vulkan support.

## Локальная разработка

```powershell
npm install
npm run tauri dev
```

### Уровни тестирования

Используйте подходящий уровень, а не installer по умолчанию:

1. **Только UI в браузере:** `npm run dev:ui`, затем
   `http://127.0.0.1:1420/?ui=v2`. Это mock-режим для быстрой визуальной и
   интерактивной проверки UI v2; Rust, Tauri IPC, микрофон, hotkey и overlay в
   нём не проверяются.
2. **Desktop без установки:** `npm run tauri dev` для разработки или прямой
   запуск `src-tauri\target\release\fono.exe` после production-сборки. Это
   настоящий Tauri-процесс с Rust-бэкендом; на нём проверяются основное и
   overlay-окна и нативные сценарии.
3. **Installer:** `npm run release` создаёт NSIS/MSI и используется для
   финального release smoke-test: упаковки ресурсов, установки и запуска в
   пользовательском окружении. Его не нужно запускать для каждой UI-итерации.

Быстрые проверки Rust:

```powershell
cd src-tauri
cargo fmt --all --check
cargo check -p fono --no-default-features
cargo check -p fono --no-default-features --features whisper-wake
cargo check -p fono --no-default-features --features sherpa-wake
cargo check -p fono
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check --config deny.toml
cargo test -p fono stt::worker::tests::base64_transport_measurement_for_typical_recording_lengths -- --nocapture
```

## Release

Единый путь release-сборки:

```powershell
npm run release
```

Если frontend и release resources уже подготовлены и проверены отдельно, повторную упаковку
можно запустить через Tauri CLI с `src-tauri/tauri.prebuilt.conf.json`; этот
override пропускает их повторную подготовку, но перед bundling удаляет только
известные legacy-артефакты прежнего имени `WhisperClone` из `target\release`.
Rust build и bundling при этом остаются обязательными.

Он запускает frontend build, затем `scripts/prepare-release-resources.ps1` и
Tauri bundle. Подготовительный скрипт очищает только generated `.exe`/`.dll` в
явно заданных staging-каталогах, пересобирает workers, отдельным release build
получает Sherpa runtime и формирует закрытый manifest. Он создаёт:

- `fono-stt-cuda-worker.exe` + CUDA runtime DLL;
- `fono-stt-vulkan-worker.exe` на актуальном `vendor/whisper.cpp`.

Не заменяйте worker-файлы вручную в installer: `build.rs` определяет реальный
Cargo output через `OUT_DIR`, проверяет release-manifest и синхронизирует workers
с `<target-dir>/<profile>/resources/stt-workers` для прямого запуска EXE. Сам
`build.rs` не меняет source tree: `tauri.conf.json` кладёт только подготовленный
manifest в bundle. Неполный или содержащий посторонние `.exe`/`.dll` набор
workers либо Sherpa runtime завершает release-сборку ошибкой.

Release использует зафиксированные `Cargo.lock` и toolchain. Политика обновления
зависимостей и лицензий находится в `docs/dependency-policy.md`.

MSI и NSIS устанавливаются для всех пользователей в `Program Files\Fono`.
Пользовательские данные (settings, модели, API token и история) остаются в
`%APPDATA%\Fono` и не служат install directory. Custom WiX template намеренно
не читает legacy HKCU `InstallDir` от прежних current-user пакетов, чтобы MSI
не попытался записать программу в каталог данных.

Готовые артефакты:

```text
src-tauri\target\release\bundle\nsis\
src-tauri\target\release\bundle\msi\
```

## Runtime smoke-test

1. Запустить Fono и выбрать модель Whisper.
2. Проверить CUDA, Vulkan, Auto и CPU через тест диктовки.
3. Убедиться, что в логах есть `STT backend selected: CUDA` либо `Vulkan`.
4. Для Sherpa скачать KWS model и проверить встроенный `LIGHT UP` WAV, затем
   ручную запись текущей wake-фразы.
   Для Sherpa доступны только `hey fono` и `okay fun`; для другой фразы
   используйте Whisper Experimental.
5. Проверить wake → post-wake dictation → Listening: микрофон не должен
   переподключаться между этими фазами, так как их обслуживает один `AudioHub`.
6. Для command hotkey и явной команды после wake phrase убедиться, что действие
   выполняется только после preview и confirm; смена настроек либо ожидание более
   30 секунд инвалидирует preview.
7. Проверить, что worker не открывает Terminal.

Логи находятся в `%APPDATA%\Fono\logs`.

## Локальный REST API

Desktop runtime запускает API на `127.0.0.1:17832`. Для одновременной работы
нескольких dev-копий до старта Fono задайте другой порт:

```powershell
$env:FONO_API_PORT = "17833"
npm run tauri dev
```

Контракт и ручной smoke-test: [LOCAL_TRANSCRIPTION_API.md](LOCAL_TRANSCRIPTION_API.md).

API key LLM хранится в Windows Credential Manager под target
`Fono/llm-api-key`; `settings.json`, IPC и диагностические логи его не содержат.
