# Fono: CI и подготовка обновлений

## Что уже подготовлено

`Fono checks` запускается при push в `main` / `codex/**`, PR в `main` и вручную.

- Frontend: locked `npm ci`, Vue typecheck, lint, актуальные Vue unit tests,
  проверки release scripts, форматирование, синхронизация версии и production build.
- Windows: `cargo fmt`, тесты всего workspace, clippy с запретом предупреждений,
  CPU / Whisper Wake / Sherpa Wake feature checks. Тесты протокола проверяют
  отмену, границы транспортных данных и lifecycle без реального GPU.
- Rust core / protocol проверяются отдельным заданием без приватного npm-пакета.
- UI Kit фиксирован на 0.6.0. Actions привязаны к commit SHA, Node — 22.23.3,
  Rust — 1.96.1. Версия Rust соответствует `src-tauri/rust-toolchain.toml`.

Автоматические задания используют обычные `ubuntu-24.04` / `windows-2022` hosted
runners; large runners и GPU runners не используются. Для публичных репозиториев
стандартные hosted runners бесплатны, для приватных действуют квоты аккаунта.
Обычный CI не сохраняет artifacts или cache: frontend повторно собирается в
Windows job. Хранение Actions artifacts делит квоту с GitHub Packages даже при
бесплатных standard runner minutes. Ручной signed-update artifact хранится сутки;
installer около 419 MB занимает большую часть Free quota 500 MB. До запуска
этого workflow проверьте свободную квоту и установите бюджет Actions/Packages
на ноль с остановкой расхода сверх бюджета. Это настройка аккаунта, которую
данные workflows не меняют. См. [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions).

## Приватный UI Kit

В `whitewolf06/fono` 2026-10-04 настроен repository secret `WHITEUI_NPM_TOKEN`
из существующей пользовательской конфигурации npm; имя и время обновления
подтверждены через API. При ротации нужен токен с `read:packages` и доступом
к `@whitelife-core/ui-kit`. Реестр задаёт `.npmrc`;
`setup-node` пишет ссылку на переменную `NODE_AUTH_TOKEN` во временный npm config.
Значение токена не записывается в репозиторий и не выводится в лог.

Для внешних fork frontend/native job пропускаются: секрет не передаётся чужому
коду. Core/protocol job выполняется. Полная проверка такого PR проводится после
проверки изменений в доверенной ветке. `pull_request_target` не используется.
Отсутствующий токен в доверенной ветке — явная ошибка, а не успешная проверка.

## Воспроизводимые native resources

`scripts/bootstrap-ci-resources.ps1` скачивает закреплённый официальный архив
Sherpa-ONNX 1.13.4, проверяет SHA-256 и структуру путей, копирует четыре DLL в
разрешённый staging-каталог и устанавливает `SHERPA_ONNX_LIB_DIR`. Проверяется
версия в `Cargo.lock` и SHA-256 Silero VAD, уже включённого в Git.

Windows-тесты запускаются через `scripts/test-native.ps1`: сначала Cargo собирает
тот же набор тестов с `--no-run` и сообщает точные пути исполняемых файлов в JSON,
затем четыре DLL из `SHERPA_ONNX_LIB_DIR` копируются рядом с каждым executable.
После этого выполняется обычный `cargo test --locked --workspace --all-targets`.
Старые копии DLL заменяются по SHA-256. Пути вне выбранного Cargo target и
неполный runtime отклоняются. Это также поддерживает `--target-dir` и target triple.
Один `PATH` недостаточен: системный `onnxruntime.dll` из `System32` имеет более
высокий приоритет и может привести к падению при несовместимой версии API.
Состав тестов и проверки нейронного VAD не сокращаются.

Локальный запуск из корня репозитория в PowerShell 7:

```powershell
./scripts/bootstrap-ci-resources.ps1
./scripts/test-native.ps1
# Только Wake/VAD, с отдельным каталогом артефактов:
./scripts/test-native.ps1 -CargoArguments @('--locked', '-p', 'fono-wake', '--features', 'sherpa-wake', '--target-dir', 'target-wake-check') -TestArguments @('--nocapture')
```

Версии / URL / хэши находятся в `scripts/ci-native-dependencies.json`:

- Sherpa: digest официального [release asset](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.4).
- CUDA 13.3.1: официальный NVIDIA installer; SHA-256 сверяется с
  [пакетным manifest Microsoft](https://github.com/microsoft/winget-pkgs/blob/master/manifests/n/Nvidia/CUDA/13.3/Nvidia.CUDA.installer.yaml).
- Vulkan SDK 1.4.350.0: URL / SHA-256 из [LunarG SDK downloads](https://vulkan.lunarg.com/sdk/home/).

Обычный CI не требует локальных CUDA/ONNX DLL разработчика. GPU workers
исключены только из resource glob CPU job через `TAURI_CONFIG` / JSON Merge
Patch; остальные ресурсы сохранены. Это позволяет проверять свежий checkout,
где GPU binaries отсутствуют. Обычная release-конфигурация остаётся полной.
GPU workers
собираются и проверяются в ручном задании подготовки обновления:
`prepare:release-resources` → `build:workers` → hello protocol/capabilities каждого
worker. Смена протокола требует обновления бинарников и успешного handshake.
Это проверка упаковки и контракта; качество распознавания на GPU требует
отдельной проверки на реальном оборудовании.

Ключ `-InstallGpuSdks` разрешён только внутри GitHub Actions. Он устанавливает
закреплённые build SDKs через тихие параметры установщиков; локально этот ключ
отклоняется. CUDA устанавливается выбранными compiler/runtime subpackages без
GPU-драйвера; Vulkan использует `copy_only=1`. Инструкции:
[NVIDIA silent installation](https://docs.nvidia.com/cuda/archive/13.3.1/cuda-installation-guide-microsoft-windows/index.html),
[LunarG unattended installation](https://vulkan.lunarg.com/doc/view/1.4.309.0/windows/getting_started.html).

Локально проверить manifest без скачивания и установки:

```powershell
./scripts/bootstrap-ci-resources.ps1 -ValidateOnly
npm run test:release
```

## Подписанные обновления

Нативный updater использует Tauri и проверяет подпись installer. Установка
вызывается только отдельным действием пользователя. Публичный канал задаёт
`src-tauri/update-channel.json` (`publicKey` / `endpoint`), переменные окружения
`FONO_UPDATER_PUBLIC_KEY` / `FONO_UPDATER_ENDPOINT` могут его переопределить
при сборке. Изменение окружения уже установленного приложения канал не меняет.
При отсутствии обоих источников канал не настроен и проверки сети нет.
Автоматическая проверка по умолчанию выключена; пользователь может включить её.
Updater signing защищает обновление; это отдельная подпись от Windows Authenticode
и не заменяет сертификат издателя / SmartScreen reputation.
Схема формата: [Tauri updater](https://v2.tauri.app/plugin/updater/).

Для ручного workflow `Prepare signed Windows update (artifacts only)`:

Сначала workflow-файл должен появиться в default branch для
[`workflow_dispatch`](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
Ветка по умолчанию репозитория — `main`; настройка подтверждена через GitHub API
2026-10-04. Локальный `origin/HEAD` также указывает на `main`.
Само задание подписанной сборки допускает только выбранную ветку `main`.

1. Создать GitHub environment `fono-update-preparation` с required reviewers.
   Разрешить deployment branch только `main`.
2. В `src-tauri/update-channel.json` хранится публичный ключ и HTTPS URL.
   Environment variables `FONO_UPDATER_PUBLIC_KEY` / `FONO_UPDATER_ENDPOINT`
   необязательны и переопределяют эти значения при сборке. Публичный ключ — **содержимое**
   `.pub` файла Tauri signer; при смене ключа нужны согласование и миграция уже
   установленных клиентов.
3. Environment secrets: `TAURI_SIGNING_PRIVATE_KEY` — содержимое приватного ключа;
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — пароль, если задан. Токен WhiteUI также
   должен быть доступен этому workflow. Ключи не генерируются задачей CI.
4. Запустить workflow на `main` и указать `download_base`: будущий HTTPS каталог
   installer текущей версии, например
   `https://github.com/whitewolf06/fono/releases/download/v0.6.12/`.

Workflow повторяет проверки, собирает NSIS с временным override
`bundle.createUpdaterArtifacts=true`, затем проверяет `.sig` против выбранного
публичного ключа и содержимого installer. Проверка поддерживает Ed25519 Minisign
`Ed` / prehashed `ED`, включая подписанный trusted comment. URL без HTTP,
credentials, query и fragment; размеры signature/JSON/installer ограничены.
`requireSignedVersion=true` требует, чтобы версия внутри подписанного trusted
comment совпадала с `package.json` и JSON. Для этого нужен Tauri CLI 2.11.5:
его bundler записывает версию, а CLI 2.11.4 её не добавлял.
При ручной подписи через CLI 2.11.5 нужен `tauri signer sign --app-version`
с версией приложения: подпись без versioned trusted comment validator отклоняет.

Выходной Actions artifact:

- NSIS `.exe` и его `.sig`;
- `latest.json`: версия, время, URL и **содержимое** signature для `windows-x86_64`;
- `latest.json.sha256`: SHA-256 installer для ручной сверки;
- `revision.txt`: исходный commit.

В workflow нет `contents:write`, создания release, загрузки в публичный канал
или установки приложения. `download_base` — будущий адрес; генерация JSON не
доказывает, что адрес доступен. Артефакты нужно скачать и проверить перед
отдельной публикацией. В опубликованном Fono 0.6.12 уже настроены согласованный
публичный ключ и endpoint.

## Бесплатный канал GitHub Releases

Пользователь выбрал существующий `whitewolf06/fono`: 2026-10-04 его visibility
изменена на public после проверки всех локальных / удалённых веток и тегов,
истории и текущих исходников на секреты. GitHub Releases должен оставаться
бесплатным. Публичный репозиторий позволяет updater читать assets без аккаунта.
GitHub Releases допускает assets меньше 2 GiB и не ограничивает суммарный размер
релиза или bandwidth — [официальные ограничения](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).
Выпускаемый installer ограничен 512 MiB нашим validator и native updater.

Порядок публикации: сначала загрузить immutable installer и `.sig` в versioned
release, проверить доступность без аккаунта, затем опубликовать `latest.json`.
Endpoint: `https://github.com/whitewolf06/fono/releases/latest/download/latest.json`.
Публичный ключ фиксируется в `update-channel.json`; приватный ключ остаётся вне
репозитория. Локальный ключ хранится в пользовательском каталоге `.fono/signing`;
его резервную копию нужно хранить отдельно от исходников. В CI ключ передаётся
только через signing secret. [Fono 0.6.12](https://github.com/whitewolf06/fono/releases/tag/v0.6.12)
опубликован 2026-10-05: локально собранный подписанный NSIS, `.sig` и `latest.json`.
Подпись, версия и доступность публичного канала без аккаунта проверены.
Этот workflow не меняет visibility и не публикует release.
Не помещать PAT / секреты в URL клиентского updater.

## Границы подтверждения

Для исходного коммита выпуска 0.6.12 `fb54dc1883baee3f287b523dc82b10b0a058f9c9`
[Fono checks #37245293432](https://github.com/whitewolf06/fono/actions/runs/37245293432)
успешно прошёл все три задания: core/protocol, frontend/release scripts и Windows
native workspace. Это не проверка пользовательского микрофона или установки.

Ручной workflow подписанной сборки остаётся отдельным этапом: локальная подпись
и обычный CI не подтверждают его выполнение. Перед его запуском нужно проверить
signing secrets, protected environment и квоты аккаунта.
Clean-Windows installer acceptance отложен пользователем;
этому этапу также остаются реальные проверки download → signature verification
→ установка → перезапуск со своей историей и настройками.
