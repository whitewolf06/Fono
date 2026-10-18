# Пятый этап improvement-plan

Срез от 18 октября 2026 года, ветка `codex/fono-stage5-20261018`, исходники v0.7.25–v0.7.27.
Основание — [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan)
и строка «5. Тесты» в `improvement-plan/01-refactoring-and-fixes.md`.
В этот этап входят CI-02, CI-03 и CI-04. Новые пользовательские возможности
F-01–F-14 относятся к другому разделу плана.

## Карточки и зависимости

| Карточка                                                 | Пункт | Предшествующие задачи                                                 | Локальный результат                                                     |
| -------------------------------------------------------- | ----- | --------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| [FONO-107](https://kvorum.gorbach-dev.ru/tasks/FONO-107) | CI-02 | OSS-02 / FONO-72, CI-01 / FONO-106                                    | Проверка Linux optional dependencies и запуск frontend без npm-секретов |
| [FONO-108](https://kvorum.gorbach-dev.ru/tasks/FONO-108) | CI-03 | Нет исходящих зависимостей; результат нужен PERF-04, ARC-09 и CODE-05 | Общий отчёт качества и задержки с проверяемым происхождением измерений  |
| [FONO-109](https://kvorum.gorbach-dev.ru/tasks/FONO-109) | CI-04 | STAB-07 / FONO-87, PERF-06 / FONO-81                                  | Отдельный native soak с mock-аудио и настоящим CPU worker               |

Контекст, родители и зависимости прочитаны из Кворума до реализации.
Предшествующие задачи находятся в `in_review`; это результат реализации,
а не приёмка владельцем. Родитель проверок — [FONO-54](https://kvorum.gorbach-dev.ru/tasks/FONO-54).

## CI-02: frontend на Linux без секретов

Перед `npm ci` проверяется полнота lock-файла: все 13 Linux optional packages
Rollup 4.62.2, точные версии, архитектуры, публичные tarball URL и SHA512.
После установки небольшой Rollup bundle вызывает настоящий native parser.
Изменять уже полный lock-файл ради этой проверки не потребовалось.

Семь контрактов проверяют потерю зависимости, неверные метаданные и попытки
пропустить обязательные шаги CI. Вместе с прежними workflow-контрактами
на Windows прошли 15/15 проверок. Job `frontend` остаётся доступным для всех PR.

Первый чистый Ubuntu/WSL2 snapshot прошёл 13/13 шагов: свежий `npm ci`,
typecheck, lint, build, format/version/docs, 273/273 UI-теста и 28/28 release scripts.
Использованы Node 22.23.3, npm 10.9.9, пустые npm-конфиги, новый HOME и cache,
`env -i`; приватный npm и пользовательские credentials не подключались.
819 файлов snapshot не изменились за время проверки. WSL имеет доступ к
файловой системе хоста и не заявляется изолированным контейнером.

Этот первый snapshot собран из stage-4 HEAD и точных CI-02 overlays до коммита
v0.7.25. Его состав и границы зафиксированы в [CI-02 receipt](evidence/stage5-ci02.json).
Фактический запуск fork PR в GitHub Actions остаётся отдельным критерием.

## CI-03: качество, задержка и происхождение данных

`scripts/stage5-quality.mjs` объединяет шесть свежих replay-вариантов:
legacy/current resampler, original/neural full-PCM VAD, fast/accurate decoding.
Один запуск связывает run ID, версию, хэш исходников, корпус и модель,
фактические EXE, CPU, backend, число потоков и decoding options.
Отчёт отклоняет смешанные источники, неполные срезы и нечисловые измерения.
WER/CER независимо пересчитываются из каждого reference/hypothesis.

Корпус содержит 20 публичных RU/EN WAV FLEURS под CC-BY-4.0: тихие и шумные,
короткие и длинные записи. Каждый из шести вариантов прошёл все 20 файлов —
120 измерений. Срезы RU/EN, quiet/noise и short/long имеют собственные
WER/CER/RTF. Сводка задержки разделяет загрузку модели, первый inference
и прогретые вызовы с p50/p95/p99. Квантили считаются по разным клипам корпуса,
а не по многократным повторам одного клипа. Имена и attribution корпуса сохранены в JSON.

| Сравнение                           | Измерение                                                     | Вывод                                                                      |
| ----------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Legacy → current resampler          | 85 → 88 ошибок на 450 слов; 18,8889% → 19,5556%, +0,6667 п.п. | Допуск CI-03 +1 п.п. выполнен; строгий PERF-04 «не хуже» остаётся открытым |
| Original → neural, полный PCM, fast | ΔWER = 0, ΔCER = 0                                            | Строгий gate ARC-09 выполнен; обрезка PCM не включается                    |
| Fast → accurate                     | 85 → 84 ошибки слов; CER 207/2846 → 262/2846                  | Общий выигрыш качества не подтверждён; fast остаётся default               |
| Fast original / neural              | RTF 0,04621 / 0,04862                                         | Наблюдение этого debug CPU запуска                                         |
| Accurate original / neural          | RTF 0,05867 / 0,06006                                         | Наблюдение этого debug CPU запуска                                         |

Загрузка модели заняла около 0,11–0,18 с, первый inference — 0,44–0,73 с.
Это dev/debug CPU на i5-12600KF при неконтролируемой внешней нагрузке.
Независимого принятого latency baseline пока нет; норматив задержки не объявлен
пройденным. Опциональный строгий latency gate требует отдельного совместимого
запуска, заданного отношения и допуска; использовать отчёт как собственный
baseline нельзя. Холодный файловый cache ОС не измерялся.

Исходные WAV имеют частоту 16 кГц. Для сравнения ресемплеров использована
замороженная линейная реконструкция 16→48→16 кГц; настоящий вход 48 кГц
этим replay не измерен. Quiet/noise получены детерминированными преобразованиями.
Срез long начинается с 10 с и включает повтор реальной реплики; это не корпус
независимой минутной речи. Эти ограничения сохранены вместе с числами.

Нормализация повторяет Rust evaluator: lowercase, Unicode Alphabetic/Number,
остальные символы становятся схлопываемыми разделителями. WER считается по
словам, CER — по Unicode code points, включая нормализованные пробелы.
NFKC не применяется; `ё` и `е` различаются.

Финальный run ID — `stage5-ci03-final-v0.7.26-4d220f161a384a30b1e4717dba1dad72`.
Source fingerprint — `a700c1c0484a09988ec8a45ca3dd6eef77ffe6aaec8fd25ad3363b2871e999d0`.
20/20 контрактов прошли. [CI-03 JSON](evidence/stage5-ci03.json) содержит все
120 исходных reference/hypothesis, независимые counts, хэши evaluator и lineage.
Первый replay и найденные при review ошибки валидации сохранены отдельно;
числа таблицы взяты из повторного финального прогона после исправлений.

## CI-04: 1000 отмен и 100 worker faults

Добавлены ручной workflow `.github/workflows/lifecycle-soak.yml`, runner
`scripts/lifecycle-soak.mjs`, PowerShell launcher и отдельный ignored Rust-тест
`stt::worker::lifecycle_soak::cpu_worker_lifecycle_soak`.
Без явной конфигурации тест завершается ошибкой. Команды запуска — в [testing](testing.md).

Mock заменяет только физический AudioRecorder и передаёт pinned публичный WAV
в настоящий bounded writer. Проверяются Pipeline, OperationCoordinator,
operation token, SttScheduler, WorkerSession, CPU Whisper и cleanup supervisor.
990 отмен выполнены до transport; 10 — после подтверждённой записи полного
Transcribe frame в stdin worker, с наблюдением CPU activity. Вход в kernel
Whisper отдельным датчиком не измеряется.

Каждый fault принудительно завершает только собственный загруженный idle helper
после успешного health. Проверяются transport failure, reap, закрытие stdio,
открытие новой WorkerSession и повторная загрузка модели. Автоматический
recovery router SttEngine этим сценарием не проверяется.

| Показатель финального v0.7.27 прогона   | Результат                                                |
| --------------------------------------- | -------------------------------------------------------- |
| Capture/start/cancel                    | 1000/1000, из них 10 active transport cancellations      |
| Worker kill / reap / restart            | 100/100/100; все собственные дочерние процессы завершены |
| Idle                                    | 61,084 с при запросе 60 с                                |
| Общее время native сценария             | 94,452 с                                                 |
| Parent handles, loaded / shutdown       | Δ0 / Δ0                                                  |
| Parent threads, loaded / shutdown       | Δ−3 / Δ−3                                                |
| Parent private bytes, loaded / shutdown | +413 696 / +1 204 224 байт, оба ниже допуска 10 МиБ      |
| Capture start p50 / p95 / p99           | 0,0573 / 0,0854 / 0,1135 мс                              |
| Cancel-to-return p50 / p95 / p99        | 7,63 / 9,6069 / 12,0611 мс                               |
| Fault reload p50 / p95 / p99            | 129,0415 / 157,9069 / 166,6667 мс                        |

Для gates сравниваются состояния после прогрева: loaded → loaded и
no-worker → shutdown при живых одинаковых Pipeline/job/watchdog.
20 прогревочных capture-циклов исключены из 1000 измеряемых.
Рост handles/threads не допускается; private bytes ограничены
`baseline + max(5%, 10 MiB)`. Приведён 31 raw resource checkpoint;
28 прогретых checkpoints входят в отдельную проверку монотонного роста.

Первый smoke 10/2 сравнивал cold pre-worker с shutdown и обнаружил +2 handles;
он сохранён как неуспешный. После перехода к требуемому планом прогретому
baseline smoke 10/2 и полный 1000/100 прошли без изменения допусков.
Cold startup разница остаётся видимой в raw samples.

Отдельный Win32 Job ограничивает потомков тестового процесса. Watchdog следит
за конкретным Node runner и deadline; остановка теста завершает собственных
потомков. Чужие процессы в job не включаются. Runner независимо пересчитывает
resource gates, проверяет числовые поля, счётчики, хэши и фактический idle.
8/8 Node-контрактов прошли, включая 11 workflow mutations.

[CI-04 receipt](evidence/stage5-ci04.json) содержит raw native report,
runner envelope, хэши driver/worker/model/WAV, hardware и неуспешный smoke.
60 секунд автоматического idle не заменяют аппаратную цель 60 минут
из [FONO-112](https://kvorum.gorbach-dev.ru/tasks/FONO-112).

## Итоговая проверка и Кворум

Финальная [anonymous Linux integration](evidence/stage5-linux-final.json)
прошла **16/16** шагов для candidate v0.7.27: UI **273/273**, release scripts
**28/28**, quality contracts **20/20**, soak contracts **8/8**, dependency/CI
contracts **15/15**, CPU contract **1 PASS / 9 Windows-only skips**.
Typecheck, lint, build, format/version/docs и Prettier всех workflows прошли.
832 исходных файла snapshot сохранили хэши; временный каталог удалён.

Snapshot — неизменяемый HEAD `17b14a0c69ffcebbee3ecafdeddfa0eff65e51b1`
плюс 19 явно перечисленных overlays. Package построен из HEAD без правок
владельца к cache hooks. Поздняя редактура текста `testing.md`, этот отчёт,
финальные native/tracker receipts находятся вне Linux snapshot и проходят
отдельные root documentation checks. Первый Linux lint failure в двух regex
сохранён; после эквивалентной замены пробелов на `{6}` весь прогон повторён.

В рабочем Windows дереве `npm run test:release` прошёл **48/48**, включая
20 отдельных cache tests владельца. Первый запуск в sandbox дал 21 отказ
из-за запретов на временные Git configs, DLL rename и symlink; он сохранён.
Повтор с доступом к тестовым fixtures прошёл без изменения кода.
Owner cache files и hooks в коммит этапа не включаются.
Команды, хэши логов и состав тестов собраны в [scripts receipt](evidence/stage5-scripts.json).

Итоговая [native-матрица v0.7.27](evidence/stage5-native-final.json) — **14/14 PASS**:
default и CPU-only по **561/561**, в каждом наборе **478** тестов приложения;
strict Clippy, шесть feature modes и оба release-check прошли.
Только отдельно выполненный тяжёлый soak исключён из каждого обычного набора.
307 native source inputs сохранили состав, хэши и mtimes между заморозкой
до первого gate и проверкой после последнего. Post-build EXE hashes сняты
после пересборки v0.7.27; они не подменяются хэшами прежних бинарников.
Штатная cache maintenance вызывалась; очистка отложена из-за активных чужих
сборок, её защитные проверки не обходились.

Кворум: [portable sync receipt](evidence/stage5-kvorum-sync.json) содержит
**12** подтверждённых и прочитанных обратно операций: **7** комментариев
и **5** переходов статуса. Результаты внесены в FONO-107/108/109, PERF-04/FONO-79,
FONO-112, родительскую FONO-54 и эпик FONO-47. Три задачи этапа находятся
в `in_review`; FONO-54/FONO-47 остаются `in_progress`, FONO-112 — `todo`.
Переходов в `done` и подтверждения владельца нет.

CI-02 зафиксирован коммитом `de12bd3a3f495e038bf9a02c5f26ad87a5fed040`
с версией v0.7.25, CI-03 — `17b14a0c69ffcebbee3ecafdeddfa0eff65e51b1`
с v0.7.26. Этот отчёт и CI-04 входят в следующий patch-коммит v0.7.27.
Его фактический hash записывается отдельным комментарием в эпик после коммита;
precommit receipt не заявляет ещё не существующий hash.

## Открытая приёмка

PERF-04 требует отдельной работы: +3 ошибки из 450 сохраняются.
Реальный fork PR/remote Actions, аппаратные GPU/CPU-only/Auto, microphone,
sleep/resume, USB unplug, окна/hotkey/clipboard и внешняя вставка остаются
открытыми. Установленная release-сборка, updater и чистая Windows проверяются
владельцем в FONO-112. WakeWord и «На лету» остаются выключенными.
V3-код в этапе не менялся; незавершённые визуальные сценарии browser-матрицы
четвёртого этапа остаются перечислены в [STATUS](STATUS.md).

В этом этапе не запускались пользовательский Fono, запись микрофона,
установка или публикация. Собственные проверочные процессы использовали
публичные WAV. Исходные правки владельца в AGENTS/development/cache scripts
сохранены отдельно от коммитов этапа.
