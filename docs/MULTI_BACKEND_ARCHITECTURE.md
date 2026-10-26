# Переключаемые STT backend-ы

Fono использует общий `SttEngine` для встроенного Whisper и отдельных
CUDA/Vulkan workers. CUDA и Vulkan компилируются внутрь `whisper.cpp`;
отдельные процессы позволяют выбирать ускорение во время работы приложения.

```text
Fono UI → Rust pipeline → SttEngine
                          +-- fono-stt-cuda-worker.exe
                          +-- fono-stt-vulkan-worker.exe
                          +-- встроенный Whisper
```

CUDA worker — Rust crate `src-tauri/crates/fono-stt-worker` на `whisper-rs`.
Vulkan worker — CMake-проект `src-tauri/crates/fono-stt-vulkan-worker`
на закреплённом `src-tauri/vendor/whisper.cpp`.

## Выбор ускорения

Кандидаты определены в `src-tauri/src/stt/paths.rs`:

| Настройка | Порядок выбора                                                             |
| --------- | -------------------------------------------------------------------------- |
| `Auto`    | CUDA worker → Vulkan worker → встроенный GPU, если собран → встроенный CPU |
| `CUDA`    | CUDA worker; при отсутствии worker — встроенная CUDA, если собрана         |
| `Vulkan`  | Vulkan worker; при отсутствии worker — встроенный Vulkan, если собран      |
| `CPU`     | Встроенный CPU                                                             |

Успех worker означает совместимый handshake и загрузку выбранной модели.
В `Auto` ошибка кандидата позволяет проверить следующий. Явный CUDA/Vulkan
не переходит на CPU и не повторяет отказавший worker через другой кандидат:
ошибка возвращается пользователю. Выбранное ускорение и фактический backend
хранятся отдельно; результат распознавания сообщает реальный `device`.

`ensure_loaded` переиспользует совместимую модель. Подготовка новой модели
или worker сериализована отдельным load gate; длительная загрузка не держит
общий routing lock. Замена публикуется после успешной подготовки,
старые ресурсы освобождаются вне routing lock. Readiness имеет состояния
`unloaded`, `loading`, `ready`, `failed`.

## Worker protocol 4

Источник контракта — `src-tauri/crates/fono-stt-protocol/src/lib.rs`.
Transport — JSON Lines через `stdin`/`stdout`; `stderr` используется
для технических логов. PCM — моно i16 little-endian в base64.
Максимальный request frame — 16 MiB, response frame — 1 MiB.

Запросы содержат `protocol_version`, уникальный `request_id` и,
для работы конкретной операции, `operation_id`. Ответы возвращают
соответствующие идентификаторы. Несовместимая версия, неверный ответ
или нарушенный размер завершаются контролируемой ошибкой.

| Запрос              | Результат                                                                         |
| ------------------- | --------------------------------------------------------------------------------- |
| `hello`             | `ready`: backend и capabilities                                                   |
| `load`              | `model_loaded` после загрузки модели                                              |
| `ping`              | `pong` для health-проверки                                                        |
| `transcribe`        | `result`: текст, длительности и backend                                           |
| `transcribe_window` | `window_result`: текст окна и временные метки слов                                |
| `cancel_request`    | Отмена `target_request_id`; целевой запрос возвращает `error` с `code: cancelled` |
| `shutdown`          | `shutting_down` и завершение процесса                                             |

`cancel_request` читается независимо от inference и не имеет отдельного
ответа. Handshake проверяет backend, версию, поддержку окон, отмены,
token timestamps и допустимые размеры frames. `load` выполняется
после handshake; одного `ready` недостаточно для готовности модели.

Настройки декодирования передаются отдельным необязательным полем `decoding`.
Старый запрос сохраняет прежние параметры конкретного backend: Rust CPU/CUDA
использует Greedy `best_of=1`, C++ Vulkan — Greedy `best_of=5`. Эта разница
существовала до настройки декодирования; без сравнительного GPU-прогона
она сохранена. Число потоков выбирается автоматически в пределах 1–8.
Точный режим включает Beam Search 5;
число потоков можно задать явно от 1 до 8. Выбор берётся из снимка настроек
конкретной операции. Улучшение WER точным режимом требует измерения на корпусе.
Capability `supports_decoding_options` проверяется внутри protocol 4:
worker без неё принимает прежний режим, а нестандартный выбор отклоняется
клиентом до отправки запроса. В точном режиме обе реализации сохраняют
vendor fallback `greedy.best_of=-1`, как `whisper-rs 0.16` при Beam Search.

Protocol 4 добавляет отдельный `glossary` в batch/window requests. Максимум —
32 канонических написания и 896 UTF-8 байт с разделителями; пустое поле не
отправляется. Термины проверяются до загрузки модели. CPU/CUDA и Vulkan
токенизируют их выбранной моделью, сохраняют целые термины и ограничивают общий
prompt 224 токенами. Контекст предыдущего окна использует оставшийся бюджет.
Владелец токенов живёт до завершения inference. Workers protocol 3 требуют
пересборки: молча игнорировать новые подсказки нельзя. Текущие greedy-пакеты
GigaAM/Parakeet публикуют `hotwords: false`; неподдерживаемые подсказки до них
не доходят. Подробности и ограничения качества — [FONO-132](archive/improvement-plan/improvement-fono132.md).

## Lifecycle и scheduling

Worker mailbox принадлежит выделенному owner thread. Длительное распознавание
не удерживает общий маршрутизатор. Health сообщает `busy`, когда идёт работа.
Мягкая отмена сохраняет процесс и модель; при зависании или повреждённом
протоколе supervisor завершает session и процесс.

API работает ограниченными окнами и уступает интерактивной диктовке.
Отмена окна ради приоритета не отменяет API-задание: оно продолжает работу
с checkpoint. Временные метки окон абсолютные, относительно sample cursor.
Экспериментальный поэтапный вывод использует тот же контракт, но в текущем
приложении скрыт; `live` нормализуется в `standard`.

Политика `resident` сохраняет загруженную GPU-модель. `adaptive` освобождает
её при устойчивом давлении памяти только в простое; следующий запрос
вызывает тот же `ensure_loaded`. Ограничения мониторинга и admission leases
описаны в [architecture.md](architecture.md).

## Сборка и границы проверки

`npm run build:workers` пересобирает оба EXE. Изменение протокола требует
обновления бинарников. `prepare:release-resources` и release-сборка
проверяют handshake каждого поставляемого worker и закрытый manifest
его EXE/DLL. Runtime DLL CUDA поставляются рядом с worker; CUDA Toolkit
и Vulkan SDK нужны машине сборки. Пользователю нужны совместимые драйверы.

В ARC-08 проверен Rust/Vulkan-кандидат. Вложенный `vulkan-shaders-gen`
не собрался с MSBuild из каталога Cargo: `FileTracker FTK1011` на пути
CompilerId длиной 264 символа. Повтор в Developer Shell дал ту же ошибку.
Rust-зависимость содержит whisper.cpp 1.8.3, C++-путь — 1.9.1; сравнение
качества без выравнивания исходников некорректно. До подготовленной замены
сохраняется C++ Vulkan worker. Команды, обоснование и альтернативы —
[ADR 0002](adr/0002-vulkan-cpp-worker.md).

Один `scripts/worker-contract.mjs` запускается против обоих EXE через
`verify-stt-workers.ps1`. Он проверяет handshake, health, отказ неверной версии,
ошибки аудио/JSON, точную границу frame с LF, восстановление, idle cancel
и shutdown. Прогон не загружает модель и не подтверждает отмену активного
inference или качество на реальной GPU.

Инструкции сборки — [development.md](development.md), native resources
и CI — [fono-ci-updates.md](fono-ci-updates.md). Handshake, unit tests
и сборка не подтверждают качество распознавания или установленный пакет
на AMD/Intel. Остались реальные проверки CPU-only, CUDA, Vulkan, Auto,
смены ускорения и отказов drivers/runtime — [roadmap.md](roadmap.md).
Прежние ограниченные cold/warm замеры сохранены
в [fono-voice-reliability.md](qa/voice-reliability.md).
