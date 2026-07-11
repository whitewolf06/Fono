# Vulkan backend: текущий статус

## Решение

Vulkan в Fono реализован отдельным `fono-stt-vulkan-worker.exe` на актуальном
`whisper.cpp`, а не через устаревший `whisper-rs/vulkan`. CUDA и Vulkan нельзя
переключать простым runtime-флагом в одном процессе: backend выбирается при
компиляции `whisper.cpp`.

## Подтверждено на рабочей машине

- CMake находит Vulkan SDK и собирает worker.
- Worker отвечает `{"type":"ready","backend":"vulkan"}`.
- `load` с `ggml-large-v3.bin` успешно завершён.
- Runtime log подтвердил вычисления на `NVIDIA GeForce RTX 5070 Ti` через
  `Vulkan0`; модель размещена на GPU.

Предыдущий блокер `cargo build --features vulkan` относится к архивному
`whisper-rs-sys 0.15` и больше не используется Vulkan-worker'ом.

## Требования

Пользователю нужен современный драйвер GPU с поддержкой Vulkan. Vulkan SDK
нужен только машине, которая собирает release.

## Что ещё проверить перед широким выпуском

1. Тот же installer на AMD GPU.
2. Тот же installer на Intel GPU.
3. Реальную диктовку с каждой картой и записью фактического backend в log.
4. Поведение `Auto`, когда CUDA или Vulkan driver недоступен.
