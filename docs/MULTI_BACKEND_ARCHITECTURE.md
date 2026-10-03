# Переключаемые STT backend-ы

## Продуктовая цель

В настройках Fono пользователь выбирает один режим:

- `Авто` — CUDA, затем Vulkan, затем CPU;
- `CUDA` — NVIDIA release;
- `Vulkan` — AMD, Intel или NVIDIA с Vulkan driver;
- `CPU` — совместимый fallback.

Выбор должен быть честным: выбранный backend либо запускается и отображается в
status, либо приложение показывает конкретную причину недоступности. Нельзя
молча выбрать CPU, если пользователь явно выбрал CUDA или Vulkan.

## Почему это не один feature toggle

CUDA и Vulkan компилируются внутрь `whisper.cpp`. Текущий `SttEngine` живёт в
Tauri process и получает только `use_gpu: bool`; он не умеет выбрать конкретный
GPU backend в уже собранном binary.

Поэтому release состоит из общего UI/координатора и отдельных side-by-side
STT workers:

```text
Fono UI + DictationCoordinator
        | JSON line protocol (stdin/stdout)
        +-- fono-stt-cuda-worker.exe
        +-- fono-stt-vulkan-worker.exe
        +-- встроенный CPU backend
```

Каждый worker содержит один скомпилированный backend. UI выбирает worker,
перезапускает его при смене режима и получает фактический runtime status.

## Worker contract (protocol 3, 2026-10-03)

Transport — ограниченные JSON Lines, PCM i16 little-endian в base64.
`hello` сообщает protocol version, поддержку окон, отмены, token timestamps
и пределы кадров. Release и `prepare:release-resources` проверяют handshake;
старые несовместимые EXE не принимаются.

`transcribe_window` возвращает слова с абсолютными sample timestamps.
`cancel_request` адресует уникальный request id и читается независимо от
inference. Мягкая отмена сохраняет процесс и модель; зависший worker
завершается supervisor-ом. Окна live/API ограничены, рабочие буферы
переиспользуются. Интерактивный STT вытесняет окно API; оно повторяется
с checkpoint после освобождения ресурса.

Полная схема и проверенные cold/warm замеры:
[fono-voice-reliability.md](fono-voice-reliability.md).

### Исторический минимальный contract

Вход (`stdin`, JSON Lines):

```json
{
  "type": "transcribe",
  "id": "uuid",
  "model_path": "...",
  "language": "auto",
  "samples_i16_base64": "..."
}
```

Выход (`stdout`, JSON Lines):

```json
{"type":"ready","backend":"cuda"}
{"type":"load","model_path":"..."}
{"type":"model_loaded","backend":"cuda"}
{"type":"result","id":"uuid","text":"...","audio_secs":1.2,"transcribe_secs":0.3,"backend":"cuda"}
{"type":"error","id":"uuid","code":"model_load","message":"..."}
```

`stderr` используется только для технических логов и не является IPC.

## Правила Auto

1. Прочитать manifests workers, поставленных рядом с приложением.
2. Попробовать CUDA worker; успехом считается `ready` и успешная загрузка
   выбранной модели.
3. Если CUDA не подходит, попробовать Vulkan worker.
4. Затем встроенный GPU backend, если он собран в оболочке Fono.
5. В последнюю очередь — встроенный CPU backend.
6. Сохранить фактический backend для UI и диагностического лога.

Ручной выбор CUDA/Vulkan не делает fallback без согласия пользователя: он
возвращает понятную ошибку и предлагает `Авто` или доступный режим.

## Критерии готовности

- CUDA smoke-test не регрессирует относительно рабочего текущего release.
- Vulkan worker собирается с актуальным `whisper.cpp` и запускается минимум на
  одной AMD или Intel GPU.
- CPU worker не подменяет явный GPU выбор.
- Installer содержит только нужные DLL каждого worker-а.
- Переключение режима не требует переустановки и не оставляет зависшие процессы.

## Основа реализации (2026-07-10)

- `crates/fono-stt-protocol` задаёт JSON Lines contract, включая `ping`, `load` и
  `transcribe`.
- CUDA worker собран на текущей рабочей цепочке `whisper-rs`; Vulkan worker —
  отдельный CMake-проект на актуальном `whisper.cpp` (`vendor/whisper.cpp`).
- Перед первой диктовкой выбранный worker получает `load`, поэтому модель не
  загружается во время записи пользователя.
- `SttEngine` выбирает `CUDA → Vulkan → embedded GPU → CPU` для `Auto`. Явный
  выбор CUDA или Vulkan возвращает ошибку, а не подменяется CPU.
- `scripts/build-stt-workers.ps1` собирает оба EXE. CUDA runtime DLL кладутся
  рядом с CUDA worker: пользователю нужен совместимый NVIDIA driver, но не CUDA Toolkit.

Оставшаяся release-проверка: прогнать готовый установщик на чистой AMD/Intel
машине и подтвердить фактический `device=Vulkan` на реальной диктовке.
