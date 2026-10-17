# Четвёртый этап improvement-plan

Срез от 18 октября 2026 года, ветка `codex/fono-stage4-20261018`, исходники v0.7.24.
Основание — [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan).
Этап меняет структуру Rust/Vue, владение состоянием и явные исходы диктовки.
Код, автоматические проверки, browser mock и приёмка владельца учитываются отдельно.

## Объём и зависимости

Соответствие карточек и зависимости взяты из импорта `improvement-plan/kvorum-import-2026-10-16.json`.
В таблице описан локальный результат, а не закрытие задачи владельцем.

| Карточка                                                                          | Пункт   | Зависимости плана              | Результат и граница приёмки                                                                                                                                                                |
| --------------------------------------------------------------------------------- | ------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [FONO-91](https://kvorum.gorbach-dev.ru/tasks/FONO-91/arc-04-ok-empty-transcript) | ARC-04  | —                              | Явный `DictationOutcome`: `completed`, `deferred`, `no_speech`, `cancelled`, `superseded`. Диагностика использует тот же исход; ошибки остаются ошибками.                                  |
| [FONO-90](https://kvorum.gorbach-dev.ru/tasks/FONO-90/arc-03)                     | ARC-03  | ARC-04                         | `OperationCoordinator` хранит состояние операции; UI получает проекцию. Дублирующий `AppState.pipeline_state` удалён. Завершение согласовано с отменой под operation fence.                |
| [FONO-86](https://kvorum.gorbach-dev.ru/tasks/FONO-86/stab-05)                    | STAB-05 | ARC-04                         | Отсутствие речи имеет отдельное представление в native event/snapshot и Vue. Поздний исход не должен менять следующую сессию. Окна и реальное аудио проверяет владелец.                    |
| [FONO-92](https://kvorum.gorbach-dev.ru/tasks/FONO-92/arc-05-lib-rs)              | ARC-05  | ARC-01, STAB-02                | `lib.rs` оставлен точкой сборки приложения; logging, startup/shutdown, IPC registry и wake lifecycle вынесены. Wake остаётся за выключенным feature.                                       |
| [FONO-93](https://kvorum.gorbach-dev.ru/tasks/FONO-93/arc-06)                     | ARC-06  | PERF-03                        | `HistoryRepository` принадлежит managed state, глобальный `HISTORY` удалён. События и privacy hooks внедрены явно; история не вызывает `application::trainer_*`.                           |
| [FONO-94](https://kvorum.gorbach-dev.ru/tasks/FONO-94/arc-07)                     | ARC-07  | ARC-03, ARC-06                 | Типы разделены по доменам, persistence отделён от `AppState`, выделены части pipeline, LLM и тесты REST API. Шесть оставшихся исключений приведены ниже.                                   |
| [FONO-95](https://kvorum.gorbach-dev.ru/tasks/FONO-95/arc-08-worker-rust-c)       | ARC-08  | —                              | Исследована Rust/Vulkan-сборка; сохранён C++ worker с конкретным обоснованием и общим исполняемым contract suite. Унификация требует исправления сборки и сравнения одинаковых исходников. |
| [FONO-96](https://kvorum.gorbach-dev.ru/tasks/FONO-96/arc-09-vad)                 | ARC-09  | ARC-02, CI-03                  | Решение о речи и границы берутся из Silero. Самостоятельная RMS-обрезка удалена. STT получает полный PCM: обе проверенные обрезки ухудшили WER. Оптимизация обрезки отложена.              |
| [FONO-100](https://kvorum.gorbach-dev.ru/tasks/FONO-100/code-04-llm)              | CODE-04 | —; интеграция относится к F-03 | Подготовлен неизменяемый ограниченный glossary snapshot и контракт токенизации. Словарь ещё не передаётся в Whisper/LLM; финальные точные замены сохранены.                                |
| [FONO-101](https://kvorum.gorbach-dev.ru/tasks/FONO-101/code-05-whisper)          | CODE-05 | CI-03                          | Быстрый/точный режим и auto/1–8 потоков проходят через настройки и протокол. Accurate — явный Beam Search 5. Прежние backend defaults сохранены; общего выигрыша качества не заявлено.     |
| [FONO-102](https://kvorum.gorbach-dev.ru/tasks/FONO-102/code-06)                  | CODE-06 | —                              | Тренер разделён на компоненты по содержанию; добавлены общие native/mock contract tests. Тренер визуально проверен в browser mock на трёх размерах.                                        |
| [FONO-103](https://kvorum.gorbach-dev.ru/tasks/FONO-103/fe-01)                    | FE-01   | —                              | Сохранён единый типизированный event hub второго этапа; проверены контракты подписки, disposal и защита от старого поколения.                                                              |
| [FONO-104](https://kvorum.gorbach-dev.ru/tasks/FONO-104/fe-02)                    | FE-02   | CODE-02                        | Общие подписи вынесены в типизированный русский каталог; native error catalog и безопасный renderer сохранены. Полный RU/EN и переключение языка относятся к F-10.                         |
| [FONO-105](https://kvorum.gorbach-dev.ru/tasks/FONO-105/fe-03)                    | FE-03   | PERF-02                        | Сохранена остановка чтений скрытого idle-окна, объединение show/focus и одно восстановление снимка. Это развитие и проверка результата второго этапа.                                      |

ARC-04 предшествует ARC-03/STAB-05; история PERF-03 — ARC-06, затем ARC-07.
ARC-05 использует feature isolation и crash log первого этапа. ARC-09 и
CODE-05 опираются на измерения CI-03. Локальный прогон не подтверждает
запуск quality workflow в GitHub Actions.

## Владение операциями, история и I/O

Состояние операции и её terminal reason принадлежат coordinator. Старые Stop,
Cancel и завершения фоновой работы проверяют исходный `operation_id`.
Результат без речи не подменяется пустым успешным transcript.

История публикует типизированные изменения без текстов. Инвалидация источников
выполняется под repository lock, durable observer — после освобождения lock.
Санитизация частных кэшей остаётся fallible pre-commit hook: при отказе
privacy write чтение источников блокируется до успешного повтора.
Это сохраняет порядок удаления, исключения и принятия позднего AI-ответа.

Остановка захвата, VAD и запись истории вынесены из async-потока в
`spawn_blocking`. Операция и снимок настроек фиксируются до очереди;
activity lease остаётся у фактической работы при отмене IPC-ожидания.
Архивирование и завершение отложенного действия сохраняют operation fence
и однократное принятие записи. Микрофон, hotkey и внешняя вставка требуют
отдельной проверки владельца на Windows.

## ARC-07: размеры модулей и исключения

`lib.rs` сокращён до 59 строк. Типы распределены между settings, models,
processing, history, commands и wake profile; persistence имеет собственный
модуль. Тесты REST API и LLM вынесены из production-файлов.
В census обычные комментарии и пустые строки учитываются, `cfg(test)` items
и отдельные test/fixture/vendor файлы исключаются. Это ориентир для ревью
связности, а не AST-метрика сложности.

| Модуль                                 | Строк вне `cfg(test)`, приблизительно | Причина сохранить целиком в этом этапе                                                                                                              |
| -------------------------------------- | ------------------------------------: | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `injection/clipboard.rs`               |                                   637 | Win32 owner thread, владение HGLOBAL, copy/restore и отмена образуют один lifecycle. Разделение требует отдельной проверки владения native handles. |
| `application/cuda_packages/mod.rs`     |                                   574 | Exact-version install, verified cache, отмена и activity lease описывают одну транзакцию установки пакета.                                          |
| `application/cuda_packages/storage.rs` |                                   561 | Reparse/ACL checks, удерживаемые guards, atomic promotion и anti-rollback составляют одну границу доверия файлов.                                   |
| `application/trainer_report/mod.rs`    |                                   529 | Замороженный снимок, агрегаты и проверка текущих источников должны сохранять согласованность кэша отчёта.                                           |
| `ipc/settings.rs`                      |                                   508 | Применение настроек и rollback сохраняют единый порядок эффектов; active/wake validation уже вынесены.                                              |
| `stt/recovery.rs`                      |                                   507 | Generation fence, crash supervisor и retirement согласованно управляют жизнью worker и публикацией модели.                                          |

Этап не объявляет, что каждый production-файл уже короче 500 строк.
Vue-компоненты тренера разделены на summary/terms/AI, dictations, plot и
breakdown; `PeriodReport` — 171 строка, `TrainerPage` — 247, `DailyChart` — 124.

## ARC-09: один источник решения о речи, полный PCM

[Финальный replay](evidence/stage4-quality.json) использует 20 закреплённых
публичных RU/EN WAV, 450 слов, 2846 символов и 235,36 секунды аудио.
Источник — `fono-fleurs-smoke-v1`, CC-BY-4.0; модель — pinned Whisper tiny.
Все 20 записей получили `full_capture_retained=true` и нулевую обрезку.
При одинаковом fast-декодировании ошибки совпали с исходным PCM:
**85/450** слов и **207/2846** символов, WER delta **0**, CER delta **0**.

Решение сохранить полный PCM принято после отрицательных результатов:

| Вариант                                                                | Word errors / 450 |      WER | Сравнение с исходным fast |
| ---------------------------------------------------------------------- | ----------------: | -------: | ------------------------- |
| Исходный PCM / финальный полный PCM                                    |                85 | 18,8889% | delta 0                   |
| [Silero + pre-roll 150 мс](evidence/stage4-quality-preroll-150ms.json) |                97 | 21,5556% | +12 ошибок; +2,6667 п.п.  |
| [Silero + pre-roll 2 с](evidence/stage4-quality-preroll-2s.json)       |                86 | 19,1111% | +1 ошибка; +0,2222 п.п.   |

Обе обрезки не прошли строгий критерий «не хуже» и не включены в итоговую
реализацию. Silero используется для допуска речи и диагностики границ;
отдельной RMS-границы обрезки больше нет. Начало записи и весь хвост передаются
в STT. Ускорение за счёт обрезки не заявляется; тайминги отдельных запусков
не служат независимым сравнением производительности.

Этот replay начинается с готовых 16 кГц WAV и не переоценивает ресемплер.
Прежний **PERF-04 / FONO-79** остаётся открытым: 88 ошибок против 85 из 450
в проверке второго этапа, **+3/450**, **+0,6667 п.п.** Допуск CI +1 п.п.
не заменяет исходный критерий. Нулевой delta этапа 4 этот результат не отменяет.

## CODE-04/05 и общий worker contract

CODE-04 подготовил основу для [F-03 / FONO-58](https://kvorum.gorbach-dev.ru/tasks/FONO-58/f-03-llm):
неизменяемые canonical terms из снимка настроек, максимум 32 термина,
896 UTF-8 bytes и 224 токена по фактическому tokenizer. Отключённый или
невалидный словарь не вызывает tokenizer. Подключение glossary к STT/LLM
в этот этап не входит; исходные тексты и финальные точные замены сохранены.

CODE-05 передаёт `DecodingOptions` через CPU, CUDA и C++ Vulkan из снимка
конкретной операции. Настройки применяются между сессиями. `fast` и auto threads
остаются defaults; явное число потоков ограничено 1–8. Сохранены прежние
параметры backend: Rust CPU/CUDA Greedy `best_of=1`, C++ Vulkan Greedy `best_of=5`.
Accurate включает Beam Search 5; его vendor greedy fallback `-1` совпадает
с `whisper-rs 0.16`.

На этом CPU-корпусе Beam дал **84** word errors вместо **85**, но **262**
character errors вместо **207**: WER 18,6667% против 18,8889%, CER 9,2059%
против 7,2734%. Это смешанный результат, поэтому accurate остаётся явным выбором
без обещания общего улучшения качества. GPU-сравнение этим прогоном не выполнено.

Protocol 3 дополнен optional `decoding` и capability `supports_decoding_options`.
Прежний запрос сохраняется; нестандартный выбор отклоняется до отправки старому
worker без capability. [ARC-08 evidence](evidence/stage4-worker-architecture.json)
фиксирует пересборку обоих EXE, **14/14** process cases для каждого и такой же
прогон точной базовой копии Vulkan. Это handshake, framing, ошибки, health,
проверка decoding options, idle cancel и shutdown без загрузки модели.
Активную inference-отмену, драйвер и GPU quality эти 14 случаев не подтверждают.

[ADR 0002](adr/0002-vulkan-cpp-worker.md) объясняет сохранение двух реализаций:
Rust/Vulkan-кандидат не собрал вложенный shader generator (`FileTracker FTK1011`,
путь CompilerId 264 символа), а версии whisper.cpp различаются: 1.8.3 и 1.9.1.
Его скорость, размер и WER не сравнивались. Новые worker hashes внесены только
в две EXE-записи native inventory; лицензии зависимостей, моделей и отложенный
WhiteUI notice не изменены.

## Проверки и оставшаяся приёмка

- Frontend: **273/273**, без fail/cancel/skip; typecheck, lint, build, version и format прошли. Регрессии проверяют восстановление следующей сессии при пропущенном `listening` и запоздалую отмену во время чтения нового снимка. [Frontend evidence](evidence/stage4-frontend.json) отделяет настоящий `NativeTrainer` со strict IPC fixture от Rust/LLM.
- Worker build и общий process contract: **14/14** CUDA, **14/14** Vulkan, **14/14** base Vulkan. CUDA producer — **12 passed, 1 skip** из-за symlink privilege; Windows junction проверен. Подробности и хэши — в worker evidence.
- Публичный CPU replay: полный PCM сохраняет fast WER/CER; два варианта обрезки отклонены. Модель, corpus/runner/trimmer hashes и все варианты находятся в quality JSON.

Финальная [native-матрица](evidence/stage4-native-final.json) прошла все **14/14**
проверок: default — **561/561**, CPU-only — **561/561**,
в каждом наборе **478** тестов приложения. Strict Clippy, шесть feature modes
и оба release-check прошли. Receipt содержит SHA логов и согласованный снимок
Rust/C++/header исходников. Ранние handshake timeout, Clippy item-order,
duplicate wake import и результаты до privacy review сохранены как lineage.

После privacy review добавлены четыре регрессии: неизменённая включённая политика
сохраняет текущую AI-работу; ошибки удаления, исключения и очистки блокируют
поздние записи до успешного повтора. Репозиторий хранит требуемый снимок записей,
поэтому посторонняя запись или повтор policy maintenance не восстанавливают
удалённые данные. `history.rs` содержит **442** строки целиком.

[Release-script evidence](evidence/stage4-scripts.json): основной состав коммита —
**28/28**, рабочее дерево с **20** ранее добавленными owner cache tests —
**48/48**. Контракты quality runner — **6/6**. Эти числа относятся
к локальным проверкам; installer, публикация и удалённый GitHub Actions не запускались.
[Frontend](evidence/stage4-frontend.json) — **273/273**;
[quality replay](evidence/stage4-quality.json) сохраняет полный PCM и нулевые fast WER/CER delta.

[Первый browser receipt](evidence/stage4-browser.json) сохранён как исторический
`blocked_partial`. После восстановления Chrome/CUA выполнен
[дополнительный прогон](evidence/stage4-browser-final.json): тренер
проверен на **720×560, 900×720 и 1440×900**. Проверены дни 0/3, периоды
7/30/90, count/density, переход фильтра к диктовкам, сортировка и выбор исходника.
AI consent сохраняется между вкладками и сбрасывается при смене периода;
AI-анализ не запускался. Настройки проверены на **720×560 и 900×720**,
дополнительно — на широком recovery tab **2560×1271**. Fast/accurate и threads
1/8/auto сохраняются и восстанавливаются. Старый browser-демопрофиль сначала блокировал сохранение
из-за legacy Wake-параметров; штатный сброс демо восстановил валидный профиль.
Исходники после автоматических проверок не менялись.

Overlay визуально проверен на **900×720**, включая безопасные переключатели
обработки/перевода и отмену. На **720×560** состояние ожидания подтверждено
через DOM, но screenshot снова завершился transport timeout. Overlay
**1440×900**, настройки на подтверждённых **1440×900** и сценарий no-speech
в браузере не проверены. Поэтому весь browser-прогон остаётся частичным;
выполненная матрица тренера и настроек
учитывается отдельно. Build, SSR и contract tests эту визуальную проверку
не заменяют. Viewport сброшен, текущая вкладка закрыта, собственный сервер
остановлен. Первая отсоединённая вкладка не закрылась через поддержанный API
из-за detached debugger и оставлена штатной очистке временных вкладок.

Владелец отдельно проверяет реальный микрофон, короткую/тихую речь, `hold`/`toggle`,
отмену, ручную обработку, overlay, clipboard и смену внешнего поля. GPU,
sleep/resume, длительный soak и чистая Windows остаются открытыми. Installer
для этапа 4, подпись, публикация и установка не выполнялись. WakeWord и
«На лету» не включены.

## Кворум

MCP bootstrap подтвердил готовый server-resolved scope. В Кворуме созданы и
прочитаны обратно **20 комментариев**, подтверждены и прочитаны **12 переходов
статуса** — всего **32/32** операций с совпадающим domain readback.
Все **14** задач этапа находятся в `in_review`; **5** родительских карточек
FONO-47/50/51/52/53 остаются в `in_progress`. CI-03 / FONO-108 получил
quality evidence и сохраняет `in_review`. FE-01/03 повторно не переводились.

[Portable receipt](evidence/stage4-kvorum-sync.json) содержит ключи, ID комментариев,
итоговые статусы, scope, время и SHA локальных confirmation/readback receipts.
Это исторический срез до дополнительного browser-прогона. Его результаты
записаны ещё в **6** комментариях FONO-86/101/102/104/53/47 и прочитаны обратно:
[browser sync receipt](evidence/stage4-kvorum-browser-sync.json). Всего до
фиксации коммита подтверждено **38/38** операций: 26 комментариев и 12 переходов.
Задачи не переводились в `done`; приёмка владельца не объявлена. Оставшиеся
browser cells, Windows/GPU/installer и ручные сценарии открыты.
