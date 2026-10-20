# ADR 0002: сохранить C++ Vulkan worker после проверки Rust-сборки

Обновлено: 18 октября 2026 года. Статус: временное решение по результатам ARC-08; унификация отложена.

## Контекст

CUDA worker собирается из Rust crate `fono-stt-worker`, Vulkan — из C++-проекта `fono-stt-vulkan-worker` через CMake и закреплённый `vendor/whisper.cpp`. В Rust crate также есть feature `vulkan`. Следовательно, сейчас поддерживаются две реализации протокола с риском расхождения отмены, лимитов и ошибок.

Историческая причина выбора C++ отдельно не установлена. Проверка ARC-08 дала конкретное ограничение текущей Windows-сборки: вложенный генератор Vulkan-шейдеров Rust-зависимости не проходит определение компилятора в длинном каталоге Cargo.

## Проверка 18 октября

Команда из `src-tauri`:

```powershell
$env:LIBCLANG_PATH = 'C:/Program Files/LLVM/bin'
cargo build --locked -p fono-stt-worker --bin fono-stt-worker --release --features vulkan --target-dir target-codex-review
```

В полученном CMake cache `GGML_VULKAN=ON`. Сборка использовала Visual Studio 18 2026, MSBuild 18.5.4, MSVC 19.50.35730.0, CMake 4.3.4 и Vulkan SDK 1.4.350.0. После первоначального запуска сборка CMake повторена обычным Windows-токеном, затем в `Launch-VsDevShell` с `-Arch amd64 -HostArch amd64 -SkipAutomaticLocation`:

```powershell
cmake --build target-codex-review/release/build/whisper-rs-sys-10a4d8d8bf24a3cb/out/build --target install --config Release --parallel 4 --verbose
```

Оба повторных запуска завершились ошибкой вложенного `vulkan-shaders-gen`. Верхнее сообщение `No CMAKE_C_COMPILER/CMAKE_CXX_COMPILER` скрывает более точную диагностику: в `CMakeConfigureLog.yaml` записана `FileTracker FTK1011` при создании `CompilerIdC.tlog/link-rc.read.1.tlog` и `link-rc.write.1.tlog`. Полный путь первого файла на проверенной машине — 264 символа. Родительский CMake уже определил оба компилятора; обычный Developer Shell вложенную ошибку не устранил.

Есть и различие исходников: `whisper-rs-sys 0.15.0` собирает whisper.cpp **1.8.3**, а закреплённый C++-путь — **1.9.1**. Одинаковый протокол не делает эти реализации эквивалентными по распознаванию. Хэши журналов и границы эксперимента — [stage4-worker-architecture.json](../evidence/stage4-worker-architecture.json).

## Решение

Сохранить работающий C++/CMake-путь Vulkan и Rust-путь CUDA. Текущий Rust-кандидат для Vulkan не собран воспроизводимо, а версии whisper.cpp расходятся. Удаление C++ сейчас убрало бы проверяемый путь поставки без подготовленной замены.

Обе реализации обязаны проходить один исполняемый process contract — `scripts/worker-contract.mjs`, вызываемый из `verify-stt-workers.ps1` и `build:workers`. Он проверяет protocol 4, capabilities, ping, неверную версию, повреждённый PCM/base64/JSON, границу frame с учётом LF, восстановление после ошибки frame, idle cancel без отдельного ответа и штатный shutdown.

Быстрый режим сохраняет и существующую разницу декодирования: Rust CPU/CUDA явно использует Greedy `best_of=1`, а прежний C++ Vulkan получал `best_of=5` из `whisper_full_default_params`. Унификация этих значений без GPU-замеров изменила бы fallback-поведение. Точный режим включается отдельно: Beam Search 5 и vendor fallback `greedy.best_of=-1` в обеих реализациях, как в `whisper-rs 0.16`.

Этот набор работает без загрузки модели. Idle cancel проверяет управляющий протокол; остановку активного inference, качество GPU и устойчивость драйвера он не подтверждает. Для этих свойств остаются отдельные проверки отмены, распознавания и ручная приёмка.

## Альтернативы

- Проверить Ninja и укороченный путь сборки как способы устранить вложенную ошибку MSBuild. Пока это гипотезы, отдельная сборка ими не выполнена.
- После воспроизводимой Rust/Vulkan-сборки выровнять whisper.cpp/model revision и сравнить протокол, активную отмену, зависимости и качество на одном корпусе. Затем можно решить, удалять ли C++.

## Последствия

До унификации поддерживаются две реализации протокола и один общий process contract. Изменения, включая `supports_decoding_options` и параметры Greedy/Beam Search, вносятся в обе реализации и требуют пересборки workers. Этот эксперимент не даёт основания сравнивать их скорость, размер или WER: Rust/Vulkan-кандидат не дошёл до inference.

Код: `scripts/build-stt-workers.ps1`, `src-tauri/crates/fono-stt-vulkan-worker`, `src-tauri/crates/fono-stt-worker/Cargo.toml`. Контракт: [STT backend-ы](../MULTI_BACKEND_ARCHITECTURE.md).
