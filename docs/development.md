# Разработка и release Fono

## Версия приложения

`package.json` задаёт версию Fono. `npm run version:check` проверяет её совпадение
с метаданными Tauri, Cargo и lock-файлами. Новая схема начинается с `0.5.0`;
актуальную версию всегда читайте из `package.json`.
В приложении показывается одна пользовательская версия; ревизии frontend/backend
доступны в раскрываемых сведениях о сборке.

Каждый новый коммит должен содержать следующую patch-версию и `vX.Y.Z` в теме:

```powershell
npm run version:bump -- patch
npm run version:check
git add <файлы задачи и версии>
git commit -m "fix(scope): описание [v0.5.1]"
```

Выполните `npm run hooks:install` один раз в локальном checkout, чтобы Git
отклонял коммит с несогласованной или неувеличенной версией. Перед публикацией
предложите следующую minor-версию и согласуйте её состав; после согласования
используйте `npm run version:bump -- minor`. Публикация выполняется отдельно.

## Требования машины сборки

- Rust toolchain из `src-tauri/rust-toolchain.toml` и Node.js 20+.
- MSVC Build Tools 2022 + Windows SDK.
- CMake и LLVM/libclang.
- CUDA Toolkit для CUDA worker.
- Vulkan SDK для Vulkan worker.

CUDA Toolkit и Vulkan SDK нужны только разработчику, который создаёт release.
Пользователь получает CUDA runtime DLL рядом с CUDA worker; для Vulkan нужен
драйвер GPU с Vulkan support.

## Локальная разработка

```powershell
git submodule update --init --recursive
npm ci
npm run dev:desktop:v3
```

### Уровни тестирования

Используйте подходящий уровень, а не installer по умолчанию:

1. **Только UI в браузере:** `npm run dev:ui`, затем
   `http://127.0.0.1:1420/v3.html`. Это mock-режим для быстрой визуальной и
   интерактивной проверки Vue-интерфейса, включая `#/overlay`; Rust, Tauri IPC,
   микрофон, hotkey и нативное окно индикатора в нём не проверяются.
2. **Desktop без установки:** `npm run dev:desktop:v3` для разработки или прямой
   запуск `src-tauri\target\release\fono.exe` после production-сборки. Это
   настоящий Tauri-процесс с Rust-бэкендом; на нём проверяются основное и
   overlay-окна и нативные сценарии.
3. **Installer:** `npm run release` создаёт NSIS/MSI и используется для
   финального release smoke-test: упаковки ресурсов, установки и запуска в
   пользовательском окружении. Его не нужно запускать для каждой UI-итерации.

### Изоляция debug-данных

Для отдельной debug-копии задайте абсолютный `FONO_TEST_DATA_DIR` и свободный
`FONO_API_PORT` до запуска. Настройки и история будут находиться в выбранном
каталоге; операции с настоящими Windows credentials и автозапуском заблокированы.
Release игнорирует тестовый каталог.

```powershell
$env:FONO_TEST_DATA_DIR = "C:\Temp\Fono-debug"
$env:FONO_API_PORT = "17833"
npm run dev:desktop:v3
```

Микрофон, глобальная клавиша и внешняя вставка остаются настоящими. Изолированный
каталог не превращает desktop в browser mock. После проверки уберите переменные
из текущего PowerShell-сеанса. Приложение и установку пользователь проверяет сам.

Автоматические команды и матрица Rust features — в [testing.md](testing.md).

## Release

Единый путь release-сборки:

```powershell
npm run release
```

Если frontend и release resources уже подготовлены и проверены отдельно, повторную упаковку
можно запустить через Tauri CLI с `src-tauri/tauri.prebuilt.conf.json`; этот
override пропускает их повторную подготовку, но перед bundling удаляет только
известные legacy-артефакты предыдущего имени приложения из `target\release`.
Rust build и bundling при этом остаются обязательными.

Он запускает frontend build, затем `scripts/prepare-release-resources.ps1` и
Tauri bundle. Подготовительный скрипт очищает только generated `.exe`/`.dll` в
явно заданных staging-каталогах, пересобирает workers, отдельным release build
получает Sherpa runtime и формирует закрытый manifest. Он создаёт:

- `fono-stt-cuda-worker.exe` + CUDA runtime DLL;
- `fono-stt-vulkan-worker.exe` на актуальном `vendor/whisper.cpp`.

Не заменяйте worker-файлы вручную в installer: `build.rs` определяет реальный
Cargo output через `OUT_DIR`, проверяет release-manifest и синхронизирует workers
с `<target-dir>/<profile>/resources/stt-workers` для прямого запуска EXE. Сам
`build.rs` не меняет source tree: `tauri.conf.json` кладёт только подготовленный
manifest в bundle. Неполный или содержащий посторонние `.exe`/`.dll` набор
workers либо Sherpa runtime завершает release-сборку ошибкой.

Release использует зафиксированные `Cargo.lock` и toolchain. Политика обновления
зависимостей и лицензий находится в `docs/dependency-policy.md`.

MSI и NSIS устанавливаются для всех пользователей в `Program Files\Fono`.
Пользовательские данные (settings, модели, API token и история) остаются в
`%APPDATA%\Fono` и не служат install directory. Custom WiX template намеренно
не читает legacy HKCU `InstallDir` от прежних current-user пакетов, чтобы MSI
не попытался записать программу в каталог данных.

Готовые артефакты:

```text
src-tauri\target\release\bundle\nsis\
src-tauri\target\release\bundle\msi\
```

## Runtime smoke-test

1. Запустить Fono и выбрать модель Whisper.
2. Проверить CUDA, Vulkan, Auto и CPU через тест диктовки.
3. Убедиться, что в логах есть `STT backend selected: CUDA` либо `Vulkan`.
4. WakeWord временно недоступен; включение детектора и проверка wake-фразы относятся к отдельной будущей задаче качества.
5. Проверить оба режима hotkey: удержание и повторное нажатие. Экспериментальная поэтапная вставка скрыта.
6. Для command hotkey убедиться, что действие выполняется только после preview и confirm; смена настроек либо ожидание более 30 секунд инвалидирует preview.
7. Проверить, что worker не открывает Terminal.

Логи находятся в `%APPDATA%\Fono\logs`.

## Локальный REST API

Desktop runtime запускает API на `127.0.0.1:17832`. Для одновременной работы
нескольких dev-копий до старта Fono задайте другой порт:

```powershell
$env:FONO_API_PORT = "17833"
npm run tauri dev
```

Контракт и ручной smoke-test: [LOCAL_TRANSCRIPTION_API.md](LOCAL_TRANSCRIPTION_API.md).

API key каждого LLM-профиля хранится в Windows Credential Manager под target
`Fono/llm-profile/<id>`. Прежний target `Fono/llm-api-key` поддерживается для
миграции; `settings.json`, IPC и диагностические логи ключей не содержат.

## Фронтенд

Единственный frontend — Vue в `src/v3/`. Корневой URL и `v3.html` открывают один интерфейс. `package-lock.json` и `npm ci` задают воспроизводимые зависимости. `npm run test:ui` запускает V3-тесты. Маршруты, слои и правила данных — в [frontend-architecture.md](frontend-architecture.md).
