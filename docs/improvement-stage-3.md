# Третий этап improvement-plan

Срез от 17 октября 2026 года, ветка `codex/fono-stage3-20261017`, исходники
v0.7.22. Основание — [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan).
Этап начат, но полностью не завершён: открытие WhiteUI и канал отдельного
CUDA-пакета ожидают решений владельца. Кворум в этой сессии не отвечает;
подтверждённых записей о третьем этапе на сервере пока нет.

## Объём и зависимости

| Карточка                                                                 | Пункт  | Локальный результат                                                                                                                                                                        |
| ------------------------------------------------------------------------ | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [FONO-72](https://kvorum.gorbach-dev.ru/tasks/FONO-72/oss-02-npm)        | OSS-02 | Точная WhiteUI 0.6.0 и provenance проверены; anonymous Linux preview проходит. Выбор vendoring и точный MIT notice ожидают владельца; production dependency не переключена.                |
| [FONO-73](https://kvorum.gorbach-dev.ru/tasks/FONO-73/oss-03-cpu-only)   | OSS-03 | CPU-only feature, launcher, Tauri override, изолированные данные и реальный speech VAD. Код и автоматические проверки готовы к ревью; чистая Windows и голос требуют владельца.            |
| [FONO-74](https://kvorum.gorbach-dev.ru/tasks/FONO-74/oss-04-readme)     | OSS-04 | EN/RU README, три скриншота и GIF, CONTRIBUTING, SECURITY и issue forms подготовлены. Ограничение приватной установки указано явно; clean source bootstrap зависит от OSS-02.              |
| [FONO-75](https://kvorum.gorbach-dev.ru/tasks/FONO-75/oss-05)            | OSS-05 | [Конкретное предложение](cuda-package-proposal.md) по base/CUDA поставке и подписи. Решение по каналу ожидается; downloader, новый base installer и официальный CUDA asset не реализованы. |
| [FONO-107](https://kvorum.gorbach-dev.ru/tasks/FONO-107/ci-02-ui-oss-02) | CI-02  | Чистый Linux preview без credentials проходит. Fork guard и WhiteUI secrets в рабочем workflow сохраняются до OSS-02; это не выполненный fork PR.                                          |
| [FONO-110](https://kvorum.gorbach-dev.ru/tasks/FONO-110/doc-01)          | DOC-01 | Разделены руководство, архитектура, QA и STATUS; старые адреса QA сохранены как переходы.                                                                                                  |
| [FONO-111](https://kvorum.gorbach-dev.ru/tasks/FONO-111/doc-02)          | DOC-02 | Четыре ADR: GPU workers, временный C++ Vulkan, заморозка WakeWord и SQLite. ARC-08 не объявлен закрытым.                                                                                   |

DOC-01/02 не имеют блокирующей реализации и выполнены первыми в локальном
коммите `7e4ad06` / v0.7.21. OSS-03 опирается на `fono-audio` первого этапа.
OSS-04 и CI-02 сохраняют зависимость от OSS-02; OSS-05 — от OSS-03 и ARC-01.
Открытая приёмка PERF-04 второго этапа не скрыта и не переопределена.

## CPU-профиль

`npm run dev:cpu` запускает Windows desktop с `cpu-only`, который включает
настоящий `speech-vad`. CUDA/Vulkan исключены из Cargo-выбора, поиска workers
и Tauri resources. Случайно найденный рядом worker не становится кандидатом;
Auto использует embedded CPU. Явно выбранный недоступный GPU возвращает ошибку.

Launcher использует отдельный debug-профиль `build/dev-cpu-profile` и порт
17833 по умолчанию. Настоящие credentials и автозапуск заблокированы правилами
существующей debug-изоляции; микрофон, hotkey и IPC остаются настоящими.
Окружение восстанавливается после выхода дочерней команды. CUDA/Vulkan SDK
и submodule Vulkan worker для этого пути не нужны.

LLVM/libclang остаётся обязательным. Проверка готовых upstream bindings
`whisper-rs-sys` выявила несовместимый Linux C ABI на Windows; этот вариант
убран без изменения crate или подавления assertions. Финальная сборка
генерирует Windows bindings. Остальные prerequisites указаны в
[development](development.md); на машине теста GPU SDK не удалялись, их
features и переменные отключались явно. Чистая машина требует отдельного прогона.

## Sherpa bootstrap

Повторный старт больше не создаёт успешные GUID-распаковки. Используется один
кэш на полный pinned SHA256. Каждый запуск проверяет архив и все реальные
файлы кэша по доверенному содержимому; marker не заменяет проверку байтов.
Первичная распаковка проходит в своём staging, проверяется целиком и атомарно
публикуется. Прерванный staging не блокирует новый запуск.

Hash и tar работают с одним ограниченным снимком архива. Проверяются размеры,
пути, exact tree, ссылки/reparse и компоненты путей записи от checkout до
archive/lock/resource/DLL. Изменённый published cache блокирует запуск и не
чинится поверх потенциально используемых DLL. Старые пользовательские GUID
каталоги и build caches не удалялись.

## WhiteUI: технический proof и граница решения

Исходный tarball настоящей WhiteUI 0.6.0: 134150 байт,
SHA256 `e0aabc07070a54eae56cfb810f120b3ebdcb12f34e1313a1eecc073c5dfaab65`.
Все 95 файлов совпадают с установленным пакетом. Тег `v0.6.0` указывает на
`2fa5232000ab20349e35703ffd8fe7f5a0188167`; generated dist в теге отсутствует,
его пересборка не заявляется. Объявлена MIT, но полного LICENSE/copyright в
исходном архиве и теге нет. Лицензия более позднего Gavia назад не переносилась.

В отдельном ignored checkout из `git archive 7e4ad06` изменены только источник
WhiteUI на локальный tarball и registry mapping. Рабочий проект не переключён.
Контейнер получал только этот scratch, пустые npm-конфиги и Linux filesystem;
host npm cache и credentials не монтировались.

[Linux evidence](evidence/stage3-linux-preview.json): Node 22.23.3, npm 10.9.9,
Debian 12 x64, glibc 2.36. `npm ci`, typecheck, lint, UI **239/239**, tracked
release scripts **21/21**, format/version/build/docs прошли; lock после
установки не менялся. Образ закреплён digest; собственный контейнер удалён.
Это внутренний preview, а не принятая лицензия или фактический fork workflow.

## Проверки итоговых исходников

- CPU-only Windows workspace `--no-default-features --features fono/cpu-only --all-targets --include-ignored` — **497/497**, без пропусков. Включены 423 app tests и публичный WAV через настоящий pinned Silero VAD.
- Обычный Windows workspace `--all-targets --include-ignored` — **495/495**, без пропусков. Pinned Sherpa DLL сверены и размещены рядом с test executables штатным wrapper.
- Strict Clippy `--workspace --all-targets -- -D warnings` для default и CPU-only, Rust fmt, шесть feature modes и CPU release-check с настоящими frontend/resources — прошли.
- Typecheck, lint, UI **239/239**, release scripts **41/41** в текущем рабочем дереве, frontend build, format/version checks — прошли. Дополнительные 20 cache tests относятся к исходным пользовательским файлам и остаются вне коммитов этапа; в чистом HEAD release suite содержит 21 тест.
- Python archive/security suite — **27/27**, launcher fixtures — **10/10**. В этих наборах проверены реальные junction вне sandbox, два успешных старта, same-size tamper, interrupted init, environment restore и сохранение exit code. Настоящее приложение fixture не запускает.
- Browser mock: главная, настройки обработки и тренер на **720×560, 900×720, 1440×900**, без горизонтального переполнения и warn/error. [Численные результаты](evidence/stage3-browser-checks.json). Три иллюстрации и GIF имеют [описание происхождения](assets/README.md).
- License inventory соответствует трём lock-файлам: 248 npm, 710 Cargo registry packages, прежние 141 evidence gap. Новые зависимости не добавлялись; обновлены хэши версионных lock-файлов.

Логи находятся локально в `build/checks/stage3-*.log`, feature summary —
`stage3-feature-matrix.json`. Первые диагностические попытки сохранены отдельно:
несовместимые готовые bindings и отсутствовавшие параметры ignored VAD fixture.
Assertions и критерии не ослаблялись. После исправления окружения полный
native-прогон выполнен заново.

## Кворум и продолжение

Bootstrap MCP и публичный HTTPS из этой сессии завершаются таймаутом. Без
server-resolved ready scope записи не выполнялись. Восстановимая локальная
очередь — `build/checks/stage3-kvorum-pending.json`; она явно помечена как
неприменённая. Серверные статусы этим отчётом не заменяются.

Для продолжения нужны два решения владельца: способ открытия WhiteUI с точным
MIT copyright/годом и канал доверия CUDA-пакета. После них остаются активация
OSS-02, fork-safe CI-02, завершение OSS-04 и реализация/проверка OSS-05.
Подписанный CUDA asset, измерение нового base installer и Windows/GPU-приёмка
являются отдельными ещё не выполненными результатами.

Микрофон, пользовательский Fono, hotkey, внешняя вставка, installer и публикация
автоматически не запускались. Ухудшение WER PERF-04 на **0,6667 п.п.** остаётся
открытым в [предыдущем отчёте](improvement-stage-2.md) и [STATUS](STATUS.md).
