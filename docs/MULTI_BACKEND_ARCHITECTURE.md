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
        +-- fono-stt-cuda.exe
        +-- fono-stt-vulkan.exe
        +-- fono-stt-cpu.exe
```

Каждый worker содержит один скомпилированный backend. UI выбирает worker,
перезапускает его при смене режима и получает фактический runtime status.

## Минимальный worker contract

Вход (`stdin`, JSON Lines):

```json
{"type":"transcribe","id":"uuid","model_path":"...","language":"auto","samples_i16_base64":"..."}
```

Выход (`stdout`, JSON Lines):

```json
{"type":"ready","backend":"cuda"}
{"type":"result","id":"uuid","text":"...","audio_secs":1.2,"transcribe_secs":0.3,"backend":"cuda"}
{"type":"error","id":"uuid","code":"model_load","message":"..."}
```

`stderr` используется только для технических логов и не является IPC.

## Правила Auto

1. Прочитать manifests workers, поставленных рядом с приложением.
2. Попробовать CUDA worker; успехом считается `ready` и успешная загрузка
   выбранной модели.
3. Если CUDA не подходит, попробовать Vulkan worker.
4. Затем CPU worker.
5. Сохранить фактический backend для UI и диагностического лога.

Ручной выбор CUDA/Vulkan не делает fallback без согласия пользователя: он
возвращает понятную ошибку и предлагает `Авто` или доступный режим.

## Критерии готовности

- CUDA smoke-test не регрессирует относительно рабочего текущего release.
- Vulkan worker собирается с актуальным `whisper.cpp` и запускается минимум на
  одной AMD или Intel GPU.
- CPU worker не подменяет явный GPU выбор.
- Installer содержит только нужные DLL каждого worker-а.
- Переключение режима не требует переустановки и не оставляет зависшие процессы.
