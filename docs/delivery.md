# Сборка и поставка Fono

Последний опубликованный выпуск —
[0.9.1](https://github.com/whitewolf06/Fono/releases/tag/v0.9.1).
Версия исходников может быть выше: изменение документации или patch-коммит
не создаёт новый публичный выпуск.

Windows Desktop поставляется через GitHub Releases. Fono Server поставляется
Linux x86_64 Docker-образами: основная база — Vulkan + CPU, CUDA — отдельный
образ. Standalone Windows GPU Server, deb/systemd и native portable-архив
не входят в эту поставку. Docker save-архив не является переносимым native-пакетом.

## Подготовка Windows

Базовый installer содержит Vulkan worker, embedded CPU, речевой VAD,
Sherpa runtime и отдельный проверяемый runtime Лиры beta.
CUDA worker и NVIDIA runtime DLL входят в дополнительный пакет.

```powershell
npm run release
```

Команда использует `src-tauri/tauri.lira.conf.json`. Подготовка ресурсов
пересобирает Vulkan с `-SkipCuda`, формирует точный `resources/stt-base`,
проверяет handshake и закрытые manifests. Наличие старого CUDA EXE в raw
`resources/stt-workers` не добавляет CUDA в базовый installer.
Несовместимые protocol/capabilities и посторонние файлы должны останавливать
проверку, а не пропускаться.

Зависимости Лиры закреплены в
[tools/wake-release/dependencies.json](../tools/wake-release/dependencies.json).
В release используются изолированный runtime, модели и notices, без личных
dev-записей и системного Python. Манифест проверяется до открытия микрофона.

Результаты NSIS/MSI находятся в `src-tauri/target/release/bundle/`.
Они устанавливаются в `Program Files\\Fono`; пользовательские данные остаются
в `%APPDATA%\\Fono`. Подготовленный EXE без установки — отдельный уровень проверки.

Prebuilt Tauri override допустим для повторной упаковки уже проверенных
frontend/resources. Он не заменяет resource checks и не добавляет Лиру сам:
сохраните её конфигурацию при упаковке соответствующего выпуска.

## CI и происхождение

[Обычный CI](../.github/workflows/ci.yml) проверяет frontend/release scripts,
Rust core, Windows CPU workspace, Linux CPU workspace и онлайн-установщик.
Frontend и native проверки не требуют приватного npm. Native CPU job использует
inert frontend для компиляции Tauri; настоящий Vue проверяется отдельно.
Feature checks не включают legacy WakeWord или экспериментальный live.

[Подготовка подписанного Windows update](../.github/workflows/prepare-update.yml)
запускается вручную на main через защищённое environment.
Она получает signing secrets, собирает и проверяет artifacts.
Она не публикует Release и не устанавливает приложение. Проверьте, что выбранная
ресурсная конфигурация соответствует составу выпуска, включая Лиру.

Записывайте commit, версии инструментов, hashes и результаты выполненных gates.
Если повторно используется бинарник, явно укажите его исходный commit
и основание совместимости; номер версии сам по себе происхождение не доказывает.
Тяжёлые проверки выполняйте в GitHub-hosted CI или согласованном build-окружении.
Локальная подготовка ссылок и документации не требует повторной сборки пакетов.

## Подпись updater и публикация Windows

Доверенный публичный канал задан в
[src-tauri/update-channel.json](../src-tauri/update-channel.json).
Endpoint — HTTPS GitHub Releases. Публичный ключ встраивается в приложение;
приватный signing key хранится вне Git или в protected environment secret.
Его нельзя выводить, сохранять в отчёты или включать в artifacts.
Смена ключа и канала требует решения о совместимости установленных клиентов.

Updater и online installer проверяют Minisign/Ed25519 подпись, включая
подписанный trusted comment с точной версией приложения. Подпись без
`--app-version` не проходит validator. Версии пакета, signed comment и manifest
должны совпадать; downgrade запрещён. Installer ограничен 512 MiB.
SHA-256, HTTPS и валидная подпись без проверки версии не заменяют этот контракт.

При публикации:

1. Выберите точные commit, версию и состав; выполните относящиеся к ним gates.
2. Получите подписи и проверьте их встроенным публичным ключом.
3. Загрузите неизменяемые installer, `.sig` и остальные assets в versioned Release.
4. Проверьте доступность файлов без аккаунта и совпадение размеров/хэшей.
5. Опубликуйте `latest.json` и проверьте канал
   `https://github.com/whitewolf06/Fono/releases/latest/download/latest.json`.

Генератор `scripts/update-manifest.mjs` формирует manifest из проверяемого
installer и будущего HTTPS `download_base`; создание файла не загружает assets.
Используйте закреплённый Tauri CLI. Подпись updater не является Authenticode
и не подтверждает Windows publisher reputation.

Автопроверка обновлений опциональна, не чаще раза в сутки; ручная доступна
отдельно. Проверка ничего не устанавливает. Загрузка и установка требуют
явного действия. `application::updates::activity` блокирует установку при
активных операциях; pending и фоновые задачи удерживают lease до фактического
завершения, даже если IPC-ожидание отменено.

## Онлайн-установщик

`tools/fono-setup` — отдельный Windows x64 workspace со статическим CRT.
Его EXE-версия берётся из корневого `package.json`.
Не меняйте CRT-профиль Whisper ради размера загрузчика.

```powershell
npm run build:setup
```

Команда проверяет Rust format/tests/strict Clippy и создаёт
`build/online-installer/FonoSetup.exe` с хэшем и build info.
`--check-only` читает канал, `--download-only` скачивает и проверяет пакет,
`--verify-package` проверяет имеющийся EXE без запуска установки.

Загрузчик использует встроенные endpoint/ключ без runtime override.
Он ограничивает размер и время запросов, блокирует общий cache, поддерживает
докачивание только при подтверждённой неизменности файла, отвергает опасные
ссылки и защищает проверенный EXE до выхода NSIS. Непроверенный файл не запускается.
UAC и мастер установки проверяет пользователь отдельно.

## Дополнительный CUDA-пакет Windows

CUDA выбирается вручную; установка пакета не меняет Auto.
Точный канал — versioned GitHub Releases и встроенный публичный ключ Fono.

Для отдельной сборки нужен CUDA Toolkit. Подготовьте CUDA worker через
`scripts/build-stt-workers.ps1 -SkipVulkan`, затем:

```powershell
python scripts/cuda-package.py prepare --workers src-tauri/resources/stt-workers --output build/cuda-package --revision 1
python scripts/cuda-package.py verify --manifest build/cuda-package/fono-cuda-windows-x86_64.json
```

Producer создаёт детерминированный плоский USTAR/bzip2 с фиксированными
метаданными. Schema 1 содержит purpose `fono-cuda`, точные app/protocol версии,
платформу `windows-x86_64`, положительную revision, размер/SHA-256 архива
и закрытый список файлов с размерами и SHA-256. Вместо произвольного URL
хранится ожидаемое имя asset.

Для версии `X.Y.Z` публикуются:

```text
fono-cuda-X.Y.Z-windows-x86_64.tar.bz2
fono-cuda-windows-x86_64.json
fono-cuda-windows-x86_64.json.sig
```

Worker, runtime DLL и полные notices/EULA должны соответствовать producer/runtime
allowlist. Конкретные имена DLL закреплены в коде пакета; смена Toolkit требует
согласованного изменения обеих allowlist и проверки лицензий.
Лимиты: 64 KiB manifest, 1 GiB archive/отдельный файл, 2 GiB распакованный payload.

Оператор подписывает точные байты manifest внешним ключом:

```powershell
node scripts/sign-cuda-package.mjs --manifest build/cuda-package/fono-cuda-windows-x86_64.json --private-key-path C:/external-secure-location/fono.key
```

Wrapper проверяет schema, версии, размеры и все файлы до подписи, затем
проверяет результат встроенным публичным ключом и signed version.
Подпись manifest аутентифицирует архив и payload через перечисленные hashes.
Подписанный набор не пересобирается на месте: создайте новый unsigned output.

Runtime последовательно проверяет подпись, purpose/platform/версии,
ограниченную загрузку, hashes/размеры, безопасную распаковку, точный состав
и production handshake. Traversal, ссылки/reparse, ADS, дубликаты, лишние
и отсутствующие файлы отклоняются.

Пакет активируется в неизменяемом slot с атомарным состоянием.
Монотонная revision запрещает downgrade и другую начинку под той же revision.
Slot от несовместимой версии Fono не исполняется. Для нового приложения нужен
официальный совместимый пакет даже при неизменном worker.

Download, verification, probe и cleanup удерживают update admission до возврата
фактической работы. Активация проходит в idle configuration transaction
с generation/epoch guards и откатом при ошибке. Recovery получает текущий
проверенный slot через сервис; модель загружается через обычный `ensure_loaded`.

Windows payload защищён OWNER RIGHTS DACL: обычный владелец может читать
и исполнять, но не добавлять DLL и не ослаблять DACL. Открытые guards защищают
файлы и каталоги пути от подмены до завершения worker/IO; перед spawn снова
проверяются seal и состав. Cleanup не следует по освобождённому имени staging.
Это не изоляция от Administrator/SYSTEM или изменения самого приложения.

## Образы API

[Container workflow](../.github/workflows/server-container.yml) собирает
Vulkan и CUDA и запускает CPU smoke для серверных PR и release tags.
Тег сам по себе не публикует образ. Публикация выполняется отдельным ручным
запуском на точном `v<version>` с `publish=true` и совпадающим полным
`confirm_sha`; только publish job получает `packages:write`.
Используется протестированный image artifact из того же run.

Публичный registry — `ghcr.io/whitewolf06/fono-server`:

- `:X.Y.Z`, `:latest`, `:vulkan`, `:X.Y.Z-vulkan` — Vulkan + CPU;
- `:cuda`, `:X.Y.Z-cuda` — отдельная CUDA-поставка.

Для воспроизводимого deployment закрепляйте digest или точную version.
Проверьте анонимное чтение manifest; не меняйте package visibility по предположению.
Публикация образа не обновляет рабочий сервер. Перед развёртыванием сохраните
конфигурацию и путь отката, затем проверьте health, авторизацию, модель/backend,
сохранность ключей и volumes. Команды находятся в [server.md](server.md).

CPU smoke подтверждает свой runtime и API. GPU, доступ драйвера, память,
worker fault/recovery и owner acceptance проверяются отдельно.
Image signing и SBOM не следует объявлять выполненными по одной публикации.
Полная Windows/Ubuntu приёмка описана в [testing.md](testing.md).
