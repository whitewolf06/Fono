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

| Режим              | Окружение                                                                                                                                                            |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI в браузере      | Node.js 22, npm                                                                                                                                                      |
| CPU desktop        | Windows x64, Node.js 22, закреплённый Rust toolchain, MSVC Build Tools с C++ x64 и Windows SDK, CMake, LLVM/libclang, PowerShell 7, Python 3.12+, curl.exe, WebView2 |
| Сборка GPU workers | Дополнительно LLVM/libclang, CUDA Toolkit для CUDA и Vulkan SDK для Vulkan; закреплённый submodule whisper.cpp                                                       |

CPU launcher генерирует Windows bindings закреплённого `whisper-rs-sys`,
поэтому LLVM/libclang остаётся обязательным. Готовые bindings из crate
используют другой C ABI и на Windows не применяются.
CUDA Toolkit, Vulkan SDK и GPU workers CPU-профилю не нужны.
Речевой VAD остаётся настоящим: bootstrap скачивает закреплённые Sherpa/ONNX
runtime и модель с проверкой SHA256. Поэтому первый запуск требует сеть.
Точные версии и хэши находятся в `scripts/ci-native-dependencies.json`.

Для интерфейса не нужен Rust. Для запуска настоящей диктовки используйте
CPU desktop; browser mock работает с демонстрационными данными.

## Локальная разработка

Минимальный desktop-путь:

```powershell
npm ci
npm run dev:cpu
```

`dev:cpu` включает отдельный feature `cpu-only` с речевым VAD, отключает GPU
в сборке и поиске workers и убирает их из Tauri resources. Даже найденный
рядом CUDA/Vulkan worker не используется; Auto выбирает embedded CPU.
Явный выбор недоступного GPU возвращает ошибку, а не меняет ускорение молча.
На старте выберите Whisper-модель и отключите обработку/перевод для проверки
диктовки без LLM. Модели Whisper не входят в Git.

По умолчанию CPU launcher использует отдельный debug-профиль
`build/dev-cpu-profile` и API-порт 17833. В нём недоступны настоящие Windows
credentials и автозапуск; микрофон, hotkey и IPC остаются настоящими.
Абсолютный `FONO_TEST_DATA_DIR` и свободный `FONO_API_PORT` можно задать заранее.
После выхода launcher восстанавливает окружение своего процесса.

Для работы над GPU или полной поставкой сначала подготовьте дополнительные
зависимости и выполните `git submodule update --init --recursive`, затем
используйте `npm run dev:desktop:v3`. Подробнее о проверках — [testing](testing.md).

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
