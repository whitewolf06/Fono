# Fono: план рефакторинга Rust-бэкенда

Статус: новый целевой план  
Дата ревизии: 2026-08-08  
Рабочая ветка: codex/backend-refactoring  
Область: src-tauri и собственные Rust-crates проекта  
Не входит в область: переписывание UI v2, изменение моделей Whisper/Sherpa, рефакторинг vendored whisper.cpp

## 0. Текущий прогресс реализации

Срез на 2026-08-08:

| Работа | Статус | Результат |
| --- | --- | --- |
| BR-001 | Частично выполнено | Введён `OperationCoordinator`: один active lease, источники UI/hotkey/wake/diagnostics, типизированные фазы и terminal events. UI/hotkey и wake-flow меняют state только по точному `operation_id`, а Pipeline публикует `operation-state`; полный перенос cleanup на operation-specific leases еще предстоит |
| BR-002 | Частично выполнено | Wake-triggered диктовка использует `WakePauseGuard`: wake listener возобновляется при любом раннем выходе и после любой await-ветки; инвариант покрыт unit-тестом. Остальные audio/overlay cleanup-пути еще предстоит собрать в leases |
| BR-003 | Частично выполнено | Удалён `StreamHolder` с `unsafe impl Send + Sync`: CPAL stream живёт на выделенном `AudioRecordingOwner` thread, Pipeline использует только start/stop commands. Оставшийся `align_to` имеет safety comment; полный audit audio conversion ещё предстоит |
| BR-004 | Выполнена первая safety-версия | Буфер ограничен 5 минутами, watchdog закрывает потерянную запись, повторный start возвращает Busy |
| BR-005 | Выполнено | Микрофон освобождается до VAD, загрузки модели и транскрипции |
| BR-006 | Частично выполнено | Supervision вынесен в `stt/worker.rs`: модуль владеет child/I/O threads, deadline покрывает stdin/stdout, timeout приводит к kill + wait, I/O threads join, stderr bounded, следующая операция перезапускает worker. Fault-injection тесты покрывают зависание, аварийный выход и битый JSON реального дочернего worker |
| BR-007 | Частично выполнено | Sherpa/Whisper/Mock имеют Drop/stop, Sherpa сохраняет JoinHandle; callback вызывается вне mutex |
| BR-008 | Частично выполнено | Настройки и история пишутся через синхронизированный временный файл; на Windows используется атомарный `ReplaceFileW` с backup предыдущей полной версии. Повреждённый или пропавший primary JSON автоматически восстанавливается из backup, сериализация операций записи защищена process-local mutex; migration/schema version остаётся отдельной работой. |
| BR-009 | Частично выполнено | Настройки проходят базовую валидацию; новые hotkey регистрируются до persistence/publication. Ошибка регистрации или записи возвращает предыдущие shortcuts, а состояние в памяти/UI публикуется только после успешной записи. Асинхронный prepare/activate/rollback wake и модели ещё предстоит. |
| BR-011 | Частично выполнено | build.rs использует OUT_DIR, release manifest fail-fast, Sherpa DLL allowlist; отдельный release-resource pipeline еще предстоит |
| BR-017 | Выполнено | Проходят no-default, Whisper-only, Sherpa-only и default configurations |
| BR-032 | Выполнено как gate | Весь workspace проходит strict Clippy с -D warnings |
| BR-033 | Частично выполнено | Cargo.lock отслеживается, toolchain закреплен; dependency audit policy еще предстоит |
| BR-030 | Частично выполнено | LLM использует один `reqwest::Client` с connection pool, connect/request deadlines, bounded JSON response (2 MiB) и отказом от пустого content. Cancellation через operation lease и HTTP fault-injection ещё предстоят. |

Промежуточные коммиты:

- bf8e35c — новый канонический план;
- 46960bd — воспроизводимая build-база и feature matrix;
- 6e031e9 — bounded recording и безопасный stop-flow;
- f88aace — strict Clippy и wake lifecycle;
- 8b84995 — worker deadlines и recovery.

Это не означает завершение этапа 1: еще нужны fault-injection stress tests, транзакционное persistence, полный wake reconfigure soak и отдельный Coordinator.

## 1. Цель

Перестроить Rust-бэкенд Fono так, чтобы он:

- имел одного явного владельца каждой пользовательской операции;
- гарантированно освобождал аудиопотоки, worker-процессы, фоновые потоки, буферы и временные файлы;
- не зависал при ошибке модели, worker-процесса, аудиоустройства, clipboard или LLM;
- не допускал параллельных конфликтующих записей и устаревших результатов;
- работал быстрее за счет устранения лишних копирований, аллокаций, блокировок и повторной инициализации;
- был тестируемым без Tauri, реального микрофона и Win32;
- сохранял совместимость с действующим UI и пользовательскими настройками во время миграции.

Рефакторинг не считается завершенным только потому, что большие файлы разбиты на модули. Главный результат — проверяемые инварианты владения ресурсами, предсказуемая машина состояний и измеримые ограничения по памяти, потокам, процессам и задержкам.

## 2. Изученные источники

Перед составлением плана изучены:

- AGENTS.md;
- README.md;
- docs/architecture.md;
- docs/development.md;
- docs/frontend-architecture.md;
- docs/MULTI_BACKEND_ARCHITECTURE.md;
- docs/MVP_HARDENING_PLAN.md;
- docs/roadmap.md;
- docs/STATUS.md;
- docs/testing.md;
- docs/VULKAN_STATUS.md;
- docs/WAKE_WORD_ARCHITECTURE.md;
- docs/ui-v2-information-architecture.md;
- kimi_review.md как исторический аудит;
- предыдущая версия этого плана;
- все собственные Rust-файлы workspace в src-tauri и crates.

Vendored-код whisper.cpp рассматривается как внешняя зависимость. Его внутренний рефакторинг не входит в этот план; проверяется только наш контракт интеграции, сборка и управление его ресурсами.

## 3. Проверенный исходный уровень

Проверки выполнены на rustc/cargo 1.96.1.

| Проверка | Результат | Значение |
| --- | --- | --- |
| cargo fmt --all --check | Пройдено | Форматирование текущего Rust-кода корректно |
| cargo test --workspace | Пройдено | 13 тестов пройдено, 1 аппаратно-зависимый тест проигнорирован |
| cargo clippy --workspace --all-targets -- -D warnings | Не пройдено | 16 предупреждений в fono-wake и 63/65 в fono |
| cargo check -p fono --no-default-features | Не пройдено | Команда безусловно ссылается на Sherpa API, отсутствующий без feature |
| Пользовательский target-dir | Нарушен | build.rs обращается к обычному src-tauri/target и fail-open продолжает сборку |
| Runtime soak с реальным микрофоном | Не выполнен | Нужна отдельная измерительная проверка |
| Проверка утечек handles/threads/processes | Не выполнена | Должна стать обязательным gate |

Текущие тесты подтверждают базовую компилируемость default-конфигурации, но не доказывают отсутствие утечек, гонок или корректность feature matrix.

## 4. Резюме аудита

### 4.1. Критические риски

1. Нет единого координатора операции. UI, hotkey, wake word, command mode и диагностические команды независимо управляют одним Pipeline.
2. Аудиопоток принудительно объявлен Send + Sync через unsafe impl, хотя фактическая модель владения CPAL stream это не доказывает.
3. Ручная запись использует неограниченный Vec. При потерянном key-up или ошибочном состоянии память может расти до завершения процесса.
4. Остановка, загрузка модели, распознавание, инъекция и восстановление wake word имеют дублирующиеся ветки cleanup. Оператор ? способен выйти до освобождения ресурса и возврата в Idle.
5. Worker-протокол использует блокирующий read_line без deadline. Зависший worker может навсегда удержать глобальный mutex STT.
6. Wake backends не имеют единой гарантии JoinHandle/Drop/shutdown. Старый поток может пережить stop или пересечься с новой конфигурацией.
7. Settings, history и загружаемые модели записываются неатомарно. Сбой процесса способен оставить поврежденный JSON или частичный файл модели.
8. build.rs копирует нативные артефакты fail-open, не уважает фактический target-dir и может упаковать устаревший worker.

### 4.2. Основные потери производительности

- wake word и диктовка открывают отдельные физические audio streams;
- callback создает несколько Vec и несколько раз копирует одни и те же samples;
- resampler не хранит фазу между callback и способен давать разрывы;
- stop_recording клонирует весь накопленный звук вместо передачи владения;
- STT передает PCM через i16 → bytes → base64 → JSON → bytes → i16 → f32;
- current_level блокирует общий writer и сканирует буфер;
- глобальный STT mutex удерживается во время долгой загрузки/распознавания;
- LLM client создается заново для каждого запроса;
- clipboard restore создает отдельный OS thread на каждую инъекцию;
- чтение логов загружает весь дневной файл для получения хвоста.

### 4.3. Архитектурные долги

- commands.rs, lib.rs и stt/mod.rs совмещают IPC, use cases, состояние, адаптеры и cleanup;
- PipelineState меняется без централизованных правил перехода;
- operation_id отбрасывает некоторые поздние результаты, но не отменяет работу и не владеет ресурсами;
- события строковые и не всегда содержат operation id;
- feature flags корневого crate не являются достоверной матрицей backend-возможностей;
- Tauri composition root включает избыточные plugins/capabilities, CSP отключен;
- Cargo.lock игнорируется, toolchain не закреплен;
- синхронные Win32 и clipboard sleeps выполняются из async-потоков;
- часть старых orchestration-функций дублирует новый путь и увеличивает число возможных состояний.

## 5. Обязательные архитектурные инварианты

Эти правила важнее конкретного расположения файлов.

### INV-01. Одна активная операция

В каждый момент времени существует не более одной операции, владеющей записью, распознаванием, command proposal или injection. Попытка начать вторую возвращает типизированный Busy с данными текущей операции, а не молча Ok.

### INV-02. Источник не равен владельцу ресурса

UI, DictationHotkey, CommandHotkey, WakeWord и Diagnostic — только источники запроса. AudioService, SttService, WakeService и InjectionService имеют собственных единственных владельцев.

### INV-03. Любой выход терминален

Success, Error, Cancelled, Timeout, DeviceLost и Shutdown проходят через один terminal path. Он:

1. запрещает новые результаты операции;
2. отменяет дочерние задачи;
3. останавливает запись;
4. завершает или убивает worker при необходимости;
5. закрывает overlay;
6. восстанавливает wake lease;
7. публикует финальное типизированное событие;
8. переводит coordinator в Idle.

### INV-04. Нет неограниченных очередей и буферов

Каждая очередь имеет capacity, политику overflow и метрику dropped/blocked. Каждая запись имеет предел по времени и числу samples. Размеры очередей задаются в миллисекундах аудио, а не случайным количеством chunks.

### INV-05. Audio callback работает в real-time режиме

После warmup callback:

- не аллоцирует heap;
- не выполняет файловый/сетевой I/O;
- не пишет обычные логи;
- не ожидает mutex, который держит другой долгий код;
- только преобразует данные в заранее выделенный буфер и отправляет их в bounded transport.

### INV-06. Отмена реальна

OperationId дополняется CancellationToken и resource lease. Устаревшая операция не только теряет право публиковать результат, но и получает сигнал остановить вычисление/worker/LLM там, где это возможно.

### INV-07. Состояние изменяет только coordinator

Адаптеры сообщают факты: AudioStarted, AudioFailed, TranscriptReady, WorkerExited. Они не меняют глобальный PipelineState самостоятельно.

### INV-08. Настройки применяются транзакционно

Новая конфигурация проходит validate → prepare → activate → persist → publish. При ошибке рабочая конфигурация и зарегистрированные hotkeys остаются прежними.

### INV-09. IPC не раскрывает секреты

Renderer получает публичный SettingsView без API keys и внутренних путей. Секреты хранятся в системном secret storage и доступны только соответствующему adapter.

### INV-10. Shutdown явный и идемпотентный

Закрытие приложения отменяет текущую операцию, останавливает audio/wake, завершает worker, дожидается фоновых потоков, сбрасывает лог-буфер и удаляет временные файлы. Повторный shutdown безопасен.

## 6. Целевая архитектура

Нужна небольшая система осмысленных модулей, а не crate на каждый тип.

    src-tauri/
      crates/
        fono-core/
          domain/
            operation.rs
            state.rs
            events.rs
            errors.rs
          application/
            coordinator.rs
            dictation.rs
            voice_command.rs
            cancellation.rs
          ports/
            audio.rs
            stt.rs
            wake.rs
            injection.rs
            persistence.rs
            llm.rs
        fono-audio/
          device.rs
          stream_owner.rs
          conversion.rs
          resampler.rs
          fanout.rs
          level.rs
        fono-wake/
          engine.rs
          matcher.rs
          backends/
        fono-stt-protocol/
        fono-stt-worker/
      src/
        app/
          runtime.rs
          composition.rs
          shutdown.rs
        ipc/
          dictation.rs
          settings.rs
          history.rs
          diagnostics.rs
          dto.rs
        adapters/
          stt/
          wake/
          injection/
          llm/
        persistence/
          settings_store.rs
          history_store.rs
          secret_store.rs
        downloads/
        observability/

### 6.1. fono-core

Чистый Rust без Tauri, CPAL, Win32, reqwest и filesystem. Содержит:

- Operation, OperationId, OperationSource, OperationPhase;
- допустимые переходы FSM;
- Coordinator и use cases;
- CancellationToken/OperationLease;
- типизированные события и ошибки;
- traits-порты для внешних сервисов;
- policy для Busy, pause, cancel, confirmation и retry.

Большая часть тестов состояния и ошибок должна выполняться здесь за миллисекунды без hardware.

### 6.2. fono-audio

Единственный thread-owned владелец CPAL stream. Wake и dictation подписываются на один нормализованный поток 16 kHz mono через bounded fan-out.

Требования:

- никаких unsafe Send/Sync для Stream;
- stream создается, управляется и уничтожается на одном owner thread;
- stateful resampler сохраняет фазу между callbacks;
- входные sample formats обрабатываются безопасно без align_to;
- буферы переиспользуются;
- device errors передаются coordinator;
- level считается инкрементально без блокировки полного dictation buffer;
- pre-roll — bounded ring buffer.

### 6.3. Адаптеры

WorkerSupervisor, WakeAdapter, WindowsInjectionAdapter, LlmAdapter и stores реализуют порты fono-core. Tauri-команды только валидируют DTO, вызывают use case и преобразуют ошибку в IPC response.

### 6.4. Composition root

AppRuntime владеет всеми долгоживущими объектами:

- coordinator task;
- audio owner thread;
- wake backend;
- worker supervisor/process;
- injection worker;
- shared HTTP client;
- tracing WorkerGuard;
- cancellation/shutdown token.

Владение видно из типов, а не скрыто в global state или mem::forget.

## 7. Реестр работ

### P0 — безопасность процесса и данных

#### BR-001. Единый OperationCoordinator

Устранить независимое управление Pipeline из IPC, hotkeys, wake callbacks и diagnostics.

Готово, когда:

- все источники вызывают один API start/cancel/confirm;
- параллельный start возвращает Busy;
- переходы FSM покрыты table-driven тестами;
- события содержат operation_id, source, phase и terminal reason.

#### BR-002. Единый cleanup и resource leases

Заменить ручные последовательности stop/set_state/hide/resume на guard/lease.

Готово, когда fault-injection в каждой await-точке оставляет Idle, закрытый overlay, остановленную запись и восстановленный wake.

#### BR-003. Убрать unsafe StreamHolder

Перенести CPAL stream на owner thread. До миграции документировать временный unsafe contract и запретить появление новых unsafe.

Готово, когда cargo geiger/ручной поиск не находит project-owned unsafe в audio lifecycle либо каждое оставшееся unsafe имеет доказанный safety comment и тест границы.

#### BR-004. Ограничить запись и все очереди

Добавить max duration, max samples и явную overflow policy. Лимит должен действовать даже при потерянном key-up.

Готово, когда тест искусственно держит запись дольше лимита и получает терминальный RecordingLimitReached без роста памяти после cleanup.

#### BR-005. Не загружать модель до остановки микрофона

В stop flow сначала захватить/остановить audio lease, затем обеспечить готовность STT. Ошибка загрузки модели не оставляет микрофон работающим.

#### BR-006. Worker deadlines и recovery

Каждый запрос worker имеет deadline. При timeout, EOF, invalid frame или crash supervisor:

- прекращает ожидание;
- собирает bounded diagnostic tail;
- kill + wait старый процесс;
- запускает новый процесс для следующей операции;
- возвращает типизированную ошибку текущей операции.

#### BR-007. Управляемый lifecycle wake threads

Каждый backend хранит JoinHandle и реализует идемпотентные stop/shutdown/Drop. Ready публикуется только после загрузки модели и успешного открытия audio subscription.

#### BR-008. Атомарное persistence

Settings и history записываются через temp file в том же каталоге, flush/sync, atomic replace и backup последней валидной версии. Добавить schema_version и миграции.

#### BR-009. Транзакционное применение settings

Проверка hotkey/wake/model выполняется до публикации новых settings. Ошибка регистрации возвращает старые runtime registrations, память и файл.

#### BR-010. Надежные downloads

Скачивание идет в staging path с progress/cancel/timeout. Перед activate проверяются размер, checksum и структура. Частичный файл никогда не считается установленной моделью.

#### BR-011. Герметичный fail-fast build.rs

build.rs обязан:

- использовать Cargo OUT_DIR/PROFILE/TARGET, а не угадывать target;
- иметь manifest ожидаемых workers/DLLs;
- копировать только нужные feature-артефакты;
- завершать сборку ошибкой при отсутствии обязательного файла;
- не модифицировать source tree;
- не использовать случайно оставшиеся артефакты старой сборки.

#### BR-012. Явный shutdown приложения

Подключить Tauri exit lifecycle к AppRuntime::shutdown с ограниченным ожиданием и логом незавершенных компонентов.

### P1 — архитектура и предсказуемость

#### BR-013. Выделить fono-core

Перенести FSM, coordinator, policies и ports в независимый crate. Tauri и Windows типы не пересекают его публичную границу.

#### BR-014. Тонкий IPC-слой

Разделить commands.rs по use cases. IPC-команды не содержат orchestration, sleep, прямой filesystem и прямые вызовы Win32.

Цель размера: обычно до 150–200 строк на cohesive module; лимит не механический, важнее одна ответственность.

#### BR-015. Типизированные события и ошибки

Убрать строковые состояния и ad-hoc payload. Версионировать event DTO. Сохранить временный compatibility mapper для UI v2.

#### BR-016. Удалить дублирующий orchestration

После миграции источников удалить run_full_pipeline, неиспользуемый start_background и старые ветки cleanup. Запретить две реализации одного flow.

#### BR-017. Корректная feature matrix

Отключить неявные default features у fono-wake и явно связать root features с backend features.

Обязательные проверки:

- no-default-features;
- whisper-only;
- sherpa-only;
- default production set;
- worker crates отдельно.

#### BR-018. Versioned worker protocol

Добавить:

- protocol_version;
- request_id/operation_id;
- capabilities handshake;
- max frame size;
- ping/health;
- shutdown;
- структурированные worker errors.

Response с чужим request_id отклоняется.

#### BR-019. Command Proposal как доменная сущность

Proposal содержит id, operation_id, source, normalized action, arguments, confidence, created_at, expires_at и settings snapshot/version.

Все источники, включая wake phrase, проходят один preview → confirm → execute. Неуверенный или неоднозначный match не исполняется.

#### BR-020. Безопасное управление секретами

Перенести API key в Windows Credential Manager или эквивалентный secret store. get_settings возвращает только has_api_key и публичные поля.

#### BR-021. Минимальные Tauri capabilities

Разделить main и overlay permissions, удалить неиспользуемые plugins, отключить withGlobalTauri, задать CSP. Overlay не должен иметь filesystem/shell/global-shortcut permissions.

#### BR-022. History как отдельный repository

Сериализовать операции append/delete/clear одним store, использовать collision-safe ID, retention policy и настройку приватности.

Ошибку записи history нельзя молча игнорировать: диктовка остается успешной, но UI получает отдельное warning event.

### P1 — производительность

#### BR-023. Один физический audio owner

Wake и dictation используют один stream и bounded subscribers. Переход wake → dictation передает lease, не закрывая и не открывая устройство заново без необходимости.

#### BR-024. Allocation-free audio hot path

Убрать Vec allocation/copy из callback, переиспользовать buffers и stateful converter. Проверять callback duration histogram и dropped frames.

#### BR-025. Передача владения recorded buffer

stop_recording возвращает накопленный buffer через mem::take/owned message, а не clone. После передачи writer сразу получает заранее выделенный новый buffer.

#### BR-026. Эффективный STT transport

Сначала измерить base64 pipeline на типичных 5/30/120 сек. Если serialization/copies превышают бюджет, перейти на:

- JSON control frames;
- length-prefixed binary PCM frames;
- явный maximum payload.

Не вводить shared memory до появления измеренной необходимости.

#### BR-027. STT concurrency без глобальной долгой блокировки

Supervisor сериализует команды через actor/mailbox, но status/health/cancel не ждут долгого transcribe mutex. Model readiness имеет отдельное состояние.

#### BR-028. Warm readiness

Фоновая загрузка модели не блокирует запись. UI получает Loading/Ready/Failed. При изменении модели новая сессия готовится до переключения, если позволяет память.

#### BR-029. Injection worker

Заменить thread::spawn на каждый clipboard restore одним выделенным worker/queue:

- одна injection за раз;
- отменяемый restore;
- проверка target HWND непосредственно перед paste;
- ограниченная задержка;
- сохранение поддерживаемых clipboard formats либо документированная безопасная деградация;
- полная проверка Win32 return values.

#### BR-030. Shared LLM client

Один reqwest::Client с connection pool, отдельными connect/request deadlines, cancellation, response size limit и проверкой пустого content. Синхронные действия не блокируют async executor.

#### BR-031. Дешевый level meter и tail logs

Level рассчитывается в аудиопотоке и публикуется через atomic/watch. Logs читаются с конца файла или из bounded ring, имеют rotation/retention.

### P2 — качество и сопровождение

#### BR-032. Очистить clippy и зависимости

Strict clippy становится обязательным. Удалить неиспользуемые зависимости/features, заменить tokio full минимальным набором. Временные allow допустимы только с issue и объяснением.

#### BR-033. Закрепить supply chain

Отслеживать Cargo.lock для desktop binary, добавить rust-toolchain.toml, cargo-deny/audit policy, licenses check и документированное обновление зависимостей.

#### BR-034. Корректная wake phrase semantics

Backend явно сообщает capabilities. Нормализация и matching тестируются для русского/английского, порядка слов, adjacency и false positives. Неподдерживаемая phrase не принимается как полностью рабочая.

#### BR-035. Временные и диагностические данные

Ограничить размер/retention логов, удалять KWS test WAV и temp downloads, очищать in-memory diagnostic samples. Полный transcript/audio не попадает в обычные логи.

#### BR-036. Документация как проверяемый контракт

После каждого этапа обновлять architecture, development, testing и feature matrix. Документация не должна утверждать shared physical audio, если фактически существуют два streams.

## 8. Порядок реализации

Нельзя начинать с массового перемещения файлов. Порядок снижает риск и дает рабочее приложение после каждого этапа.

### Этап 0. Baseline и воспроизводимые gates

Цель: сделать текущее состояние измеримым.

Работы:

1. Добавить rust-toolchain.toml и отслеживаемый Cargo.lock.
2. Исправить feature wiring настолько, чтобы матрица честно компилировалась либо явно документировать временно неподдерживаемую комбинацию.
3. Очистить strict clippy.
4. Сделать build.rs fail-fast и target-dir-safe.
5. Добавить CI: fmt, clippy, tests, feature matrix, dependency policy.
6. Добавить instrumentation для operation id, resource counters, threads/processes и stage latency без текста диктовки.
7. Снять baseline CPU, latency, memory, handles и threads.

Gate:

- все автоматические проверки зеленые;
- production build создается из чистого target directory;
- отсутствующий worker гарантированно ломает сборку;
- baseline сохранен в docs/performance-baseline.md с hardware/OS/model.

### Этап 1. Stop-the-bleeding

Цель: закрыть наиболее опасные утечки до изменения архитектуры.

Работы:

1. Busy вместо молчаливого Ok.
2. Max recording duration/samples.
3. Остановка audio до ensure_loaded.
4. Один временный terminal cleanup helper.
5. Worker request timeout и kill/wait.
6. JoinHandle для wake threads.
7. Атомарные settings/history.
8. Валидация command arguments, особенно volume_step.
9. Confirmation для wake-command.

Gate:

- 1000 start/cancel/error cycles не оставляют активную запись;
- потерянный hotkey release завершается по лимиту;
- model load failure и worker hang возвращают Idle;
- settings rollback сохраняет старые hotkeys и wake runtime.

### Этап 2. fono-core и Coordinator

Цель: создать единственный источник истины операции.

Работы:

1. Ввести core types и ports.
2. Реализовать FSM и resource leases.
3. Мигрировать источники последовательно:
   - manual UI;
   - dictation hotkey;
   - command hotkey;
   - diagnostics;
   - wake word.
4. Сохранить IPC compatibility mapper.
5. Удалять старый путь сразу после миграции каждого источника.

Gate:

- model-based/property tests не находят недопустимых переходов;
- fault test каждой фазы возвращает ресурсы;
- UI v2 проходит desktop smoke без изменения пользовательского контракта;
- старый orchestration удален.

### Этап 3. fono-audio и единый stream

Цель: безопасное владение микрофоном и быстрый callback.

Работы:

1. Owner thread и command channel.
2. Safe format conversion.
3. Stateful resampler.
4. Preallocated pool/ring buffers.
5. Bounded fan-out для wake/dictation/level.
6. Миграция dictation.
7. Миграция wake и удаление CPAL ownership из fono-wake.
8. Обработка device lost/reconnect.

Gate:

- project audio path не использует unsafe Send/Sync;
- один physical input stream подтвержден diagnostics;
- callback не аллоцирует после warmup;
- device unplug не зависает и не оставляет operation active;
- 60-минутный wake soak проходит ресурсные лимиты.

### Этап 4. STT supervisor и protocol v2

Цель: worker не может повесить приложение.

Работы:

1. Actor-based WorkerSupervisor.
2. Handshake/version/capabilities.
3. Request deadlines, request IDs, cancel и shutdown.
4. Bounded stderr tail.
5. Health/restart/fallback policy.
6. Warm readiness.
7. Измерение и при необходимости binary audio frame.

Gate:

- kill/hang/malformed-response tests проходят;
- 100 crash/restart cycles не оставляют child processes/handles;
- response другого operation_id не попадает в UI;
- latency не хуже baseline больше допустимого порога.

### Этап 5. Wake lifecycle

Цель: wake backend можно безопасно включать, выключать и менять.

Работы:

1. Ready handshake.
2. Idempotent stop/shutdown/Drop.
3. Callback вне внутренних locks.
4. Pause lease с reset session/pre-roll.
5. Prepare-then-swap конфигурации.
6. Реальный pre-roll для каждого backend.
7. Capability-aware phrase matcher.

Gate:

- 100 enable/disable/reconfigure cycles;
- не более одного backend task;
- старое аудио не вызывает detection после resume;
- ошибка новой конфигурации сохраняет старый рабочий backend.

### Этап 6. Persistence, downloads и secrets

Цель: падение процесса не повреждает пользовательские данные и не раскрывает секрет.

Работы:

1. Versioned atomic stores и migrations.
2. Транзакционное применение settings.
3. Serialized history repository, retention/privacy.
4. Credential Manager adapter.
5. Staged verified downloads.
6. Log/temp retention.

Gate:

- kill-process tests во время записи файла восстанавливают последнюю валидную версию;
- renderer никогда не получает secret;
- partial/corrupt model не активируется;
- параллельные history операции не теряют entries.

### Этап 7. Injection, commands и LLM

Цель: безопасная интеграция с внешними приложениями.

Работы:

1. Dedicated injection worker.
2. Clipboard guard и target-window validation.
3. Typed Proposal с TTL/confidence.
4. Единый confirmation flow.
5. Общий HTTP client, deadlines/cancel/limits.
6. Проверка всех Win32 результатов и ограничение arguments.

Gate:

- 1000 injection attempts не увеличивают число threads;
- clipboard restore не пересекается между операциями;
- expired/ambiguous proposal не выполняется;
- закрытие приложения отменяет LLM request за ограниченное время.

### Этап 8. Tauri boundary и hardening

Цель: root crate становится composition root, а renderer получает минимум полномочий.

Работы:

1. Разнести IPC по use cases.
2. Ввести AppRuntime и явный shutdown.
3. Удалить прямые adapter-вызовы из commands.
4. Сузить capabilities main/overlay.
5. Удалить неиспользуемые plugins.
6. Включить CSP и отключить global Tauri object.

Gate:

- overlay имеет только необходимые события/window permissions;
- core tests не требуют Tauri;
- IPC contract tests проходят;
- desktop main/overlay smoke проходит.

### Этап 9. Release qualification

Цель: доказать качество не только unit-тестами.

Работы:

1. Полный soak/performance/leak matrix.
2. Hardware matrix минимум на двух audio formats/devices.
3. Worker/model matrix.
4. Upgrade/migration test существующих settings/history.
5. Tauri production executable smoke.
6. Только затем NSIS/MSI installer verification.

Gate:

- все бюджеты раздела 9 соблюдены;
- нет P0/P1 дефектов;
- manual acceptance main/overlay/hotkeys/wake/history/settings подтвержден;
- installer проверен отдельно от desktop dev.

## 9. Бюджеты производительности и ресурсов

Числа ниже — стартовые regression gates. На этапе 0 они уточняются по измеренному baseline и фиксируются вместе с hardware. Ослабление бюджета требует результата benchmark, а не ощущения.

### 9.1. Audio callback

- 0 heap allocations после warmup;
- p99 времени callback меньше 10% доступного callback period;
- 0 blocking I/O и 0 ожиданий долгого mutex;
- dropped frames = 0 в нормальном режиме;
- overflow всегда измеряется и публикуется, а не скрывается.

### 9.2. Idle wake soak

После warmup в течение 60 минут:

- число app threads и OS handles не имеет монотонного роста;
- активен ровно один audio owner и не более одного wake backend task;
- private bytes drift не больше baseline + 10 MiB;
- очередь аудио не накапливает задержку;
- CPU p95 фиксируется и не регрессирует больше чем на 10% без объяснения.

### 9.3. Operation cycles

После 1000 коротких start/cancel и 100 error injections:

- нет активной Operation;
- нет orphan worker process;
- threads/handles возвращаются к baseline;
- private bytes возвращается в пределах baseline + max(5%, 10 MiB);
- overlay закрыт, wake lease восстановлен.

### 9.4. Worker recovery

После 100 hang/crash/restart:

- 0 orphan child processes;
- 0 незавершенных blocking reads;
- каждый запрос завершается success/error/timeout;
- новый запрос после сбоя способен создать здоровую сессию.

### 9.5. Latency

Измерять отдельно:

- button/hotkey → recording started;
- stop → audio captured;
- audio captured → transcript ready;
- transcript → injection complete;
- wake detected → dictation listening;
- command confirmed → command result.

Для каждого stage хранить p50/p95/p99. Рефакторинг не должен ухудшать p95 больше чем на 10% без согласованного обмена на надежность. First-run model load измеряется отдельно от warm path.

### 9.6. Память аудио

- recording buffer имеет вычисляемый верхний предел;
- pre-roll ring имеет фиксированный размер;
- worker frame имеет maximum payload;
- queue capacity выражена в миллисекундах;
- длинная запись либо chunked, либо завершается явным лимитом;
- копирование полного PCM допускается только на измеренной границе и отражается benchmark.

## 10. Проверки утечек и зависаний

### Автоматические

- unit tests Drop/shutdown/idempotency;
- Loom-модели для coordinator/lease, где применимо;
- proptest для FSM и protocol frames;
- fault injection после каждого resource acquisition;
- child-process integration tests с mock worker: normal, slow, hang, crash, malformed, oversized;
- allocator counters для audio callback benchmark;
- process metrics snapshot до/после stress tests;
- cargo clippy strict, cargo test, feature matrix;
- Miri для чистых unsafe-free модулей и оставшихся unsafe boundaries, где поддерживается.

### Windows runtime

- Process Explorer/Windows Performance Recorder для handles, threads, private bytes и CPU;
- Rapid hotkey press/release, включая потерянный release;
- переключение audio device во время записи;
- sleep/resume Windows;
- unplug USB microphone;
- worker kill во время transcribe;
- закрытие приложения во время recording/STT/LLM/injection/download;
- 60 минут wake idle и серия wake detections;
- clipboard с text/image/files перед injection;
- main и overlay окна отдельно.

### Обязательная интерпретация

Успешный cargo test не означает, что runtime leak test пройден. Успешный desktop smoke не означает, что installer проверен. В отчете всегда отдельно указывать:

1. static checks;
2. unit/integration;
3. desktop Tauri runtime;
4. soak/performance;
5. installer.

## 11. Стратегия миграции и совместимость

- Использовать strangler pattern: новая реализация появляется за существующим IPC facade.
- Мигрировать один источник операции за раз.
- Не держать два активных production path после завершения миграции источника.
- Сохранять DTO/event compatibility до отдельной синхронной миграции UI.
- Для settings/history вводить read-old/write-new migration с backup.
- Каждый этап — отдельная серия небольших коммитов с зеленым gate.
- Не смешивать архитектурный перенос и изменение пользовательского поведения в одном коммите.
- Feature flag для rollback допустим временно, но должен иметь дату удаления.

Рекомендуемый формат коммитов:

1. tests: зафиксировать старое поведение или воспроизвести дефект;
2. refactor: ввести boundary/port без смены поведения;
3. feat/fix: перевести один flow;
4. chore: удалить старый путь и обновить docs.

## 12. Definition of Done всего рефакторинга

Рефакторинг завершен только если одновременно выполнено:

- все операции проходят через fono-core Coordinator;
- нет project-owned unsafe для передачи CPAL stream между потоками;
- нет неограниченных buffers/queues;
- каждый долгоживущий thread/process/task имеет owner, shutdown и join/wait;
- cancellation и timeout покрывают STT, LLM, downloads и ожидания фоновых сервисов;
- settings/history/downloads устойчивы к прерыванию записи;
- секреты не хранятся в обычном settings JSON и не уходят renderer;
- strict clippy, fmt, tests и feature matrix зеленые;
- build из чистого target воспроизводим и fail-fast;
- main/overlay capabilities минимальны;
- performance и leak budgets пройдены;
- desktop Tauri smoke подтвержден вручную;
- installer проверен как отдельный финальный уровень;
- architecture/testing/development docs соответствуют коду.

## 13. Первый исполнимый пакет работ

Первый пакет не должен сразу создавать все целевые crates. Он формирует безопасную опору:

1. BR-011: исправить build.rs и проверить clean target.
2. BR-017/032/033: feature matrix, strict clippy, Cargo.lock, toolchain.
3. Добавить resource/latency instrumentation без transcript content.
4. BR-004/005: ограничить запись и останавливать audio до model load.
5. BR-006: timeout + kill/wait worker.
6. BR-007: JoinHandle и подтвержденный shutdown wake.
7. Добавить stress harness для start/cancel, worker crash и wake restart.
8. Снять baseline и только после этого начать BR-013 Coordinator.

Причина такого порядка: coordinator нужно строить поверх надежных resource primitives и воспроизводимых тестов. Иначе новая архитектура унаследует старые зависания, а улучшение производительности и памяти останется недоказуемым.
