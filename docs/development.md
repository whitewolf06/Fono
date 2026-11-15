# Разработка Fono

В репозитории находятся Windows Desktop на Tauri/Vue и отдельный headless
Fono Server. Для работы с исходниками не нужен аккаунт Кворума.
Начните с [архитектуры](architecture.md) и [правил кода](code-rules.md);
[проверки](testing.md) и [выпуск](delivery.md) описаны отдельно.

## Workspace и зависимости

Корневые `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` и `.cargo/config.toml`
задают общий Rust workspace. Библиотеки и workers находятся в `crates/`,
Desktop — в `src-tauri/`, Server — в `apps/fono-server/`.
Версии общих зависимостей и профили сборки находятся в корне.

Cargo по умолчанию пишет в `src-tauri/target`. Для отдельного прогона задайте
абсолютный `CARGO_TARGET_DIR`; не смешивайте чужие и свои временные сборки.
C++ Vulkan worker собирается через CMake отдельно от Rust workspace.
`tools/fono-setup` — самостоятельный workspace со своим lockfile и статическим
CRT; его Cargo-команды запускаются из этого каталога.

Frontend устанавливается командой `npm ci` из публичного npm.
Точный архив WhiteUI **0.6.0** находится в
[vendor/whiteui](../vendor/whiteui/README.md). Приватный npm-токен не требуется.
Версию UI Kit и его содержимое не меняйте вместе с обычной правкой Fono.

## Выберите путь запуска

| Задача                | Окружение                                                                                                                              | Запуск                                             |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Интерфейс в браузере  | Node.js 22, npm                                                                                                                        | `npm run dev:ui`                                   |
| Настоящий CPU Desktop | Windows x64, Node.js 22, закреплённый Rust, MSVC C++ x64/Windows SDK, CMake, LLVM/libclang, PowerShell 7, Python 3.12+, curl, WebView2 | `npm run dev:cpu`                                  |
| Desktop с GPU         | Окружение Desktop, Vulkan SDK; для отдельной CUDA-сборки также CUDA Toolkit                                                            | Подготовка workers, затем `npm run dev:desktop:v3` |
| Linux Server          | Ubuntu и зависимости из Linux CI или Docker                                                                                            | [Руководство сервера](server.md)                   |

Точные версии и хэши native-зависимостей закреплены в
[scripts/ci-native-dependencies.json](../scripts/ci-native-dependencies.json)
и workflow. Используйте их вместо случайного системного ONNX runtime.
На Windows CPU тоже нужен libclang: готовые bindings Whisper имеют другой ABI.

### Только интерфейс

```powershell
npm ci
npm run dev:ui
```

Откройте `http://127.0.0.1:1420/v3.html`. Это Vue V3 с mock-портами
и демонстрационными данными. Микрофон, глобальная клавиша, Rust IPC и внешняя
вставка здесь не работают. Предпросмотр индикатора доступен через `#/overlay`.

### Настоящий CPU Desktop

```powershell
npm ci
npm run dev:cpu
```

Launcher получает проверенные по SHA-256 Sherpa/ONNX-ресурсы, включает speech VAD
и `cpu-only`, исключает GPU workers и использует изолированный debug-профиль
`build/dev-cpu-profile` с API-портом 17833. Первый bootstrap требует сеть.
Модель выбирается и загружается отдельно в приложении; для первой диктовки
можно выключить LLM-обработку и перевод.

В этом профиле микрофон, hotkey и Tauri настоящие. Windows credentials
и автозапуск недоступны. CUDA Toolkit и Vulkan SDK CPU-пути не нужны.

### GPU Desktop

Подготовьте SDK нужных backend и закреплённый submodule:

```powershell
git submodule update --init --recursive
powershell -NoProfile -File scripts/build-stt-workers.ps1 -SkipCuda
npm run dev:desktop:v3
```

`-SkipCuda` собирает основной Vulkan worker. `npm run build:workers` собирает
оба backend и требует оба SDK. CUDA в приложении активируется через проверенный
дополнительный пакет; наличие EXE в raw build-каталоге не заменяет установку
пакета. Ресурсы полной поставки, runtime Лиры и упаковка описаны в
[delivery.md](delivery.md).

Перед сменой dev/установленного приложения завершите предыдущий Fono:
защита от второго экземпляра иначе передаст управление уже работающему процессу.
Выбирайте конкретный EXE и проверяйте его версию.

## Изоляция debug-данных

Для отдельной debug-копии задайте абсолютный каталог и свободный API-порт:

```powershell
$env:FONO_TEST_DATA_DIR = "C:\Temp\Fono-debug"
$env:FONO_API_PORT = "17833"
npm run dev:desktop:v3
```

Настройки и история попадут в этот каталог; операции с настоящими credentials
и автозапуском заблокированы. Release игнорирует тестовый каталог.
После проверки уберите переменные из своего PowerShell-сеанса.
Этот режим не подменяет настоящий микрофон, hotkey или вставку.

## Версии и коммиты

Источник версии — `package.json`. `npm run version:check` сверяет его
с `package-lock.json`, Tauri config, Cargo manifest и записью `fono` в lockfile.
Каждый новый коммит поднимает patch ровно на один и содержит `vX.Y.Z` в теме:

```powershell
npm run hooks:install
npm run version:bump -- patch
npm run version:check
git add <изменённые файлы и файлы версии>
git commit -m "docs(scope): описание [vX.Y.Z]"
```

Внутренние версии crates не следуют автоматически за версией приложения.
Minor и публикация согласуются отдельно. Более высокая версия в исходниках
не означает новый опубликованный выпуск.

## Изменение зависимостей

Обновляйте зависимости вместе с lockfile, notices и закреплёнными ресурсами.
Для Rust используйте `--locked`, проверьте feature matrix, строгий Clippy
и `cargo deny --config deny.toml check`. Учитывайте native-библиотеки,
лицензии, advisories, build scripts и новые дубли major-версий.

Исключение advisory должно указывать владельца, причину и срок в `deny.toml`.
Прежний успешный аудит не подтверждает сегодняшнюю базу advisories.
Недоступный инструмент или база означает непроведённую проверку.
Условия сторонних компонентов находятся в
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) и `licenses/`.

## Документация и рабочий контекст

Действующее поведение хранится в основных руководствах рядом с кодом.
План большого изменения и промежуточные результаты относятся к временной
`.work/`; правила жизненного цикла этого контекста задаёт
[AGENTS.md](../AGENTS.md). Статус задач, приоритеты и итоги конкретных прогонов
ведутся отдельно от руководств. Не добавляйте новый постоянный MD после каждой
итерации и не превращайте инструкцию в журнал прошедших тестов.

Не публикуйте модели, секреты, личное аудио, пользовательские профили или
signing key. В описании изменения укажите поведение, выполненные проверки
и оставшуюся приёмку; подробности — в [testing.md](testing.md).
