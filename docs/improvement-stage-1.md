# Первый этап improvement-plan

Срез от 17 октября 2026 года. Ветка `codex/fono-stage1-20261017`, версия исходников `0.7.10`. Работа относится к [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan). Результаты задач передаются на ревью; эпик и семь родительских story остаются в работе, поскольку следующие этапы ещё впереди.

Это локальная реализация. Выпуск, установка и ручная приёмка Windows выполняются отдельно.

## Реализация

| Задача                                                                                            | Результат и коммиты                                                                                             |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| [FONO-71](https://kvorum.gorbach-dev.ru/tasks/FONO-71/oss-01) · OSS-01                            | MIT кода и документации, отдельные условия бренда/иконки, реестр зависимостей и полные notices; `v0.7.10`.      |
| [FONO-106](https://kvorum.gorbach-dev.ru/tasks/FONO-106/ci-01-crate-fork-pr-frontend) · CI-01     | Native job независим от приватного frontend и npm-токена; `dbaa4db`, `7a7b732`.                                 |
| [FONO-82](https://kvorum.gorbach-dev.ru/tasks/FONO-82/stab-01) · STAB-01                          | Краткие потери пакетов переживаются; счётчик привязан к записи и operation ID; `b9ae4d0`, `114cb06`, `7a7b732`. |
| [FONO-83](https://kvorum.gorbach-dev.ru/tasks/FONO-83/stab-02-catch-unwind-panic-abort) · STAB-02 | Release abort сохранён, безопасный crash-log и атомарный FFI callback; `9cdd9e6`.                               |
| [FONO-76](https://kvorum.gorbach-dev.ru/tasks/FONO-76/perf-01-tauri) · PERF-01                    | Тяжёлые IPC вынесены в blocking с сохранением admission lease и порядка операций; `7a7b732`.                    |
| [FONO-89](https://kvorum.gorbach-dev.ru/tasks/FONO-89/arc-02-fono-audio-fono-wake) · ARC-02       | Общие AudioHub/capture/VAD выделены в fono-audio; `114cb06`.                                                    |
| [FONO-88](https://kvorum.gorbach-dev.ru/tasks/FONO-88/arc-01) · ARC-01                            | Wake/live исключены из default Rust/frontend, сохранена совместимость настроек; `7a7b732`.                      |
| [FONO-97](https://kvorum.gorbach-dev.ru/tasks/FONO-97/code-01) · CODE-01                          | Комментарии приведены к текущей архитектуре; `e4e1d16`.                                                         |
| [FONO-98](https://kvorum.gorbach-dev.ru/tasks/FONO-98/code-02) · CODE-02                          | Стабильные коды и общий безопасный русский каталог ошибок; `bace42f`, `7a7b732`.                                |
| [FONO-99](https://kvorum.gorbach-dev.ru/tasks/FONO-99/code-03) · CODE-03                          | Состояние, значок и меню трея; Pause привязана к ID до очереди; `7a7b732`, `b9b7037`.                           |
| [FONO-87](https://kvorum.gorbach-dev.ru/tasks/FONO-87/stab-07) · STAB-07                          | Single-instance, startup gate, отложенное создание состояния, foreground permission; `7a7b732`.                 |
| [FONO-142](https://kvorum.gorbach-dev.ru/tasks/FONO-142/w-01) · W-01                              | Удалены старые Whisper/KWS detectors и словари фраз, заморожен research-контур; `3a024c8`.                      |

ARC-02 выполнен перед ARC-01, затем W-01. CODE-02 предшествует изменению трея. Связанные границы IPC, managers, features и трея интегрированы вместе в `7a7b732`. Независимое ревью затем обнаружило гонку Pause/Resume; исправление вынесено в `b9b7037`.

По решениям владельца код и документация получили MIT, бренд и значки описаны отдельно; `panic=abort` сохранён; код «На лету» остаётся за выключенным `experimental-live`. WakeWord по-прежнему недоступен. Старые backend-идентификаторы, фраза и профиль читаются; сохранённое включение нормализуется в false. Устаревшие backend не запускаются.

## Проверки

- `typecheck:v3`, `lint`, `build`, `format:check`, `version:check` — прошли.
- `test:v3` — **218/218**. Отдельные проверки каталога ошибок — 5/5, настройки — 11/11.
- `test:release` — **41/41** в рабочем каталоге, включая существующие пользовательские cache-тесты. Их исходные изменения оставлены отдельно от этих коммитов.
- Windows native workspace/all-targets — **442/442**, без пропусков. Включён тест VAD с публичным JFK WAV и поставляемой Silero-моделью; микрофон не использовался.
- `cargo fmt --all --check` и strict Clippy workspace/all-targets с `-D warnings` — прошли.
- Пять вариантов `cargo check --no-default-features --all-targets` — прошли: без features; `wake-word`; `sherpa-wake`; `experimental-live`; `experimental-live,sherpa-wake`.
- В research-сборке дополнительно выполнены тест сохранности legacy-профиля (1/1) и точной настроенной фразы/команд (2/2).
- Default normal dependency tree не содержит `fono-wake`. В 18 production JS-файлах нет исключённых wake/live IPC, каналов и UI chunks.
- CUDA и Vulkan workers пересобраны; оба прошли hello handshake protocol 3. Это подтверждает протокол и capabilities, без STT-прогона на GPU.
- Browser mock проверен при 720×560, 900×720 и 1440×900: главная, настройки активации и preview индикатора. Ошибок консоли и горизонтального переполнения не обнаружено.
- `cargo check --release --locked --offline -p fono` с настоящими frontend/resources — прошёл; installer не создавался.

Логи текущего локального прогона находятся в `build/checks/stage1-*.log`; worker-хэши — в `build/checks/stage1-worker-hashes.json`. Для native unit tests использовались pinned Sherpa DLL и inert CI frontend; production frontend проверен отдельной сборкой. Исходные пользовательские изменения кэшей сохранены вне коммитов этапа.

## Оставшаяся приёмка

1. Владелец проверяет настоящий Windows UI, второй запуск/фокус, трей, Pause/Resume, микрофон, hotkey, внешнюю вставку и overlay по [инструкции](fono-voice-reliability.md). Автоматического управления Fono и записи микрофона не было.
2. CI-01 требует фактического fork PR и проверки намеренно падающего backend-теста в GitHub Actions. Локально проверены автономный job, mutation-контракты workflow и весь native workspace.
3. Для OSS-01 остаются полный WhiteUI copyright/permission notice, подтверждение состава и условий предоставления исходников Sherpa/eSpeak и notices в конкретном дистрибутиве. Реестр содержит 248 npm и 702 Cargo registry packages и 135 пробелов доказательств; это весь lock scope, включая dev/optional/другие платформы. Условия и границы — в [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md) и [документе бренда](brand-assets.md).
4. Подписанный installer, публикация и чистая Windows-приёмка не выполнялись.

В PERF-01 устранена синхронная работа тяжёлых Tauri-команд в UI event loop. Отдельные синхронные фазы внутри уже асинхронных lifecycle-путей (Stop/VAD, настройки, подготовка аналитики и download preflight) остаются предметом дальнейшей оптимизации async runtime; они отражены в комментарии задачи.
