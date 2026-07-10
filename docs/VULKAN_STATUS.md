# Vulkan backend: статус эксперимента

## Цель

Отдельный release Fono с `whisper-rs/vulkan` для видеокарт AMD, Intel и
NVIDIA с Vulkan driver. CUDA release остаётся основным вариантом для NVIDIA.

## Что подтверждено

- На Windows Vulkan является cross-vendor backend `whisper.cpp`.
- Для конечного пользователя нужен актуальный Vulkan-capable GPU driver, а не
  Vulkan SDK.
- Текущий UI корректно показывает Vulkan только если backend встроен в данный
  release.

## Текущий блокер

Текущая зависимость `whisper-rs 0.16` использует `whisper-rs-sys 0.15`.
Этот upstream архивирован и является последней версией на crates.io.

При `cargo build --release --features vulkan` на Windows binding собирает
вложенный `vulkan-shaders-gen` проект из `whisper.cpp`. Этот проект не проходит
MSVC/CMake try-compile. В результате Vulkan release не создаётся, хотя Vulkan
SDK и `glslc` доступны и parent CMake находит Vulkan.

Не выпускать Vulkan binary до успешных проверок:

1. `cargo build --release --features vulkan`;
2. запуск на чистой Windows с AMD или Intel GPU;
3. реальная диктовка с runtime log `device=Vulkan`;
4. проверка зависимостей installer-а.

## Безопасный путь продолжения

1. Заменить архивный binding на поддерживаемую интеграцию актуального
   `whisper.cpp` либо поддерживаемый Rust binding с Windows/Vulkan CI.
2. Сохранить адаптер `SttEngine`, чтобы остальной код (wake word, VAD,
   injection) не зависел от выбранного backend-а.
3. Вести Vulkan в отдельной ветке и не менять CUDA release до полного
   runtime smoke-test.
