# Fono: CI и подготовка обновлений

## Обычный CI

[`Fono checks`](../.github/workflows/ci.yml) запускается при push в `main` /
`codex/**`, PR в `main` и вручную. Workflow содержит четыре задания:

- Frontend: locked `npm ci`, Vue typecheck, lint, актуальные Vue unit tests,
  проверки release scripts, форматирование, синхронизация версии и production build.
- Windows: `cargo fmt`, тесты всего workspace, clippy с запретом предупреждений,
  CPU / Whisper Wake / Sherpa Wake feature checks. Тесты протокола проверяют
  отмену, границы транспортных данных и lifecycle без реального GPU.
- Rust core / protocol проверяются отдельным заданием без приватного npm-пакета.
- Онлайн-установщик проверяется отдельным Windows job: Rust format/tests/strict
  Clippy и release EXE со статическим CRT. Приватный UI Kit не нужен;
  загрузчик и NSIS не запускаются. Инструкции: [online-installer.md](online-installer.md).

Actions привязаны к commit SHA. Версии Node и Rust закреплены в workflow;
Rust должен соответствовать `src-tauri/rust-toolchain.toml`, UI Kit —
`package.json` и lockfile.

Автоматические задания используют обычные `ubuntu-24.04` / `windows-2022` hosted
runners. Обычный CI не сохраняет artifacts или cache: frontend повторно
собирается в Windows job. Ручной signed-update artifact хранится сутки.
Перед его запуском проверьте доступное место и настройки бюджета
[Actions/Packages](https://docs.github.com/en/billing/concepts/product-billing/github-actions).
Workflow не меняет настройки оплаты аккаунта.

## Приватный UI Kit

Frontend и native job требуют repository secret `WHITEUI_NPM_TOKEN`
с `read:packages` и доступом к `@whitelife-core/ui-kit`. Реестр задаёт `.npmrc`;
`setup-node` пишет ссылку на переменную `NODE_AUTH_TOKEN` во временный npm config.
Значение токена не записывается в репозиторий и не выводится в лог.

Для внешних fork frontend/native job пропускаются: секрет не передаётся чужому
коду. Core/protocol и online installer job выполняются. Полная проверка такого PR
проводится после проверки изменений в доверенной ветке. `pull_request_target` не используется.
Отсутствующий токен в доверенной ветке — явная ошибка, а не успешная проверка.

## Воспроизводимые native resources

`scripts/bootstrap-ci-resources.ps1` скачивает закреплённый официальный архив
Sherpa-ONNX, проверяет SHA-256 и структуру путей, копирует четыре DLL в
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

Версии, URL и хэши Sherpa, CUDA и Vulkan SDK находятся в
[`scripts/ci-native-dependencies.json`](../scripts/ci-native-dependencies.json).
Обновляйте их вместе с соответствующими зависимостями и проверяйте происхождение
каждого архива; правила ревью — [dependency-policy.md](dependency-policy.md).

Обычный CI не требует локальных CUDA/ONNX DLL разработчика. GPU workers
исключены только из resource glob CPU job через `TAURI_CONFIG` / JSON Merge
Patch; остальные ресурсы сохранены. Это позволяет проверять свежий checkout,
где GPU binaries отсутствуют. Обычная release-конфигурация остаётся полной.
GPU workers собираются и проверяются в ручном задании подготовки обновления:
`prepare:release-resources` → `build:workers` → hello protocol/capabilities каждого
worker. Смена протокола требует обновления бинарников и успешного handshake.
Это проверка упаковки и контракта; качество распознавания на GPU требует
отдельной проверки на реальном оборудовании.

Ключ `-InstallGpuSdks` разрешён только внутри GitHub Actions. Он устанавливает
закреплённые build SDKs через тихие параметры установщиков; локально этот ключ
отклоняется. CUDA устанавливается выбранными compiler/runtime subpackages без
GPU-драйвера; Vulkan использует `copy_only=1`. Параметры установки находятся в
[`scripts/bootstrap-ci-resources.ps1`](../scripts/bootstrap-ci-resources.ps1).

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
Автоматическая проверка по умолчанию выключена; после включения попытки идут
не чаще раза в сутки с сохранением времени между запусками. Ручная проверка
доступна независимо. Поведение и ручные сценарии —
[fono-memory-updates-qa.md](fono-memory-updates-qa.md).
Updater signing защищает обновление; это отдельная подпись от Windows Authenticode
и не заменяет сертификат издателя / SmartScreen reputation.
Схема формата: [Tauri updater](https://v2.tauri.app/plugin/updater/).

Для ручного workflow
[`Prepare signed Windows update (artifacts only)`](../.github/workflows/prepare-update.yml):

Сначала workflow-файл должен появиться в default branch для
[`workflow_dispatch`](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
Само задание подписанной сборки допускает только `refs/heads/main`.

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
4. Прочитать версию из `package.json`, запустить workflow на `main` и указать
   `download_base`: будущий HTTPS каталог installer этой версии. Например,
   для версии `X.Y.Z` —
   `https://github.com/whitewolf06/fono/releases/download/vX.Y.Z/`.

Workflow повторяет проверки, собирает NSIS с временным override
`bundle.createUpdaterArtifacts=true`, затем проверяет `.sig` против выбранного
публичного ключа и содержимого installer. Проверка поддерживает Ed25519 Minisign
`Ed` / prehashed `ED`, включая подписанный trusted comment. URL без HTTP,
credentials, query и fragment; размеры signature/JSON/installer ограничены.
Validator требует, чтобы версия внутри подписанного trusted comment совпадала
с `package.json` и JSON. Используйте закреплённый в `package.json` Tauri CLI.
При ручной подписи нужен `tauri signer sign --app-version`
с версией приложения: подпись без versioned trusted comment validator отклоняет.

Выходной Actions artifact:

- NSIS `.exe` и его `.sig`;
- `latest.json`: версия, время, URL и **содержимое** signature для `windows-x86_64`;
- `latest.json.sha256`: SHA-256 installer для ручной сверки;
- `revision.txt`: исходный commit.

В workflow нет `contents:write`, создания release, загрузки в публичный канал
или установки приложения. `download_base` — будущий адрес; генерация JSON не
доказывает, что адрес доступен. Артефакты нужно скачать и проверить перед
отдельной публикацией.

## Бесплатный канал GitHub Releases

Канал Fono использует публичный `whitewolf06/fono` и GitHub Releases.
Assets должны читаться без аккаунта; канал должен оставаться бесплатным.
Выпускаемый installer ограничен 512 MiB нашим validator и native updater.

Порядок публикации: сначала загрузить immutable installer и `.sig` в versioned
release, проверить доступность без аккаунта, затем опубликовать `latest.json`.
Endpoint: `https://github.com/whitewolf06/fono/releases/latest/download/latest.json`.
Публичный ключ фиксируется в `update-channel.json`; приватный ключ остаётся вне
репозитория. Резервную копию приватного ключа нужно хранить отдельно от исходников.
В CI ключ передаётся только через signing secret.
Этот workflow не меняет visibility и не публикует release.
Не помещать PAT / секреты в URL клиентского updater.

## Границы подтверждения

Результат CI подтверждает только проверенную ревизию и выполненные задания.
Прошлый успешный run не подтверждает последующие изменения, работу микрофона
или установку на компьютере пользователя.

Ручной workflow подписанной сборки, публикация release, установка и приёмка
на чистой Windows — отдельные этапы. Перед signed workflow проверяются
signing secrets, protected environment и квоты аккаунта. Перед публикацией —
подпись, версия и доступность файлов без аккаунта. Реальные download → проверка
подписи → установка → перезапуск со своей историей и настройками проверяются
пользователем; приёмка на чистой Windows остаётся отложенной.
