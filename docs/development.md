# Разработка и release Fono

## Требования машины сборки

- Rust 1.75+ и Node.js 20+.
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

Быстрые проверки Rust:

```powershell
cd src-tauri
cargo test -p fono-wake --features "whisper-wake sherpa-wake"
cargo check -p fono
```

## Release

Единый путь release-сборки:

```powershell
npm run release
```

Он запускает frontend build, `scripts/build-stt-workers.ps1`, затем Tauri
bundle. Скрипт создаёт:

- `fono-stt-cuda-worker.exe` + CUDA runtime DLL;
- `fono-stt-vulkan-worker.exe` на актуальном `vendor/whisper.cpp`.

Не заменяйте worker-файлы вручную в installer: `build.rs` синхронизирует их с
`target/<profile>/resources/stt-workers` для прямого запуска EXE, а
`tauri.conf.json` кладёт их в bundle.

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
5. Проверить, что worker не открывает Terminal.

Логи находятся в `%APPDATA%\Fono\logs`.
