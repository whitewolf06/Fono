# Fono Server

Самостоятельный HTTP-сервер распознаёт аудио без Tauri, микрофона и рабочего стола. Встроенная панель на / или /panel/ предоставляет вход по токену, статус, модели, очередь, ключи приложений и пробный запрос. Контракты — в [API](api.md).

## Поставка и GPU

Готовая поставка — Linux x86_64 Docker на основе Ubuntu. Основные теги ghcr.io/whitewolf06/fono-server:0.9.1 и :0.9.1-vulkan содержат Vulkan + CPU. Отдельный :0.9.1-cuda предназначен для NVIDIA CUDA. latest, vulkan и cuda обновляемые; для воспроизводимости закрепляйте версию или digest. Моделей, ключей и сертификатов в образах нет.

Windows CPU-сервер проверяется в CI, но готовой Windows GPU-поставки и службы нет. WSL2/Docker Desktop не объявляются аппаратно проверенным окружением.

models.device = "auto" выбирает поддержанную физическую дискретную Vulkan GPU, иначе CPU. Программные и интегрированные Vulkan-устройства исключены из Auto. Явный vulkan требует GPU; ошибка загрузки/выделения памяти не вызывает скрытого повтора на CPU. GPU используется для Whisper, offline Sherpa-модели работают на CPU. [STT](stt.md) описывает протоколы и ограничения.

Хост предоставляет драйвер и доступ к устройству. Для NVIDIA Vulkan через CDI нужны графические библиотеки, для AMD/Intel — /dev/dri и права render-группы. Испытания NVIDIA не подтверждают все AMD/Intel. CUDA требует совместимых драйвера, NVIDIA Container Toolkit и CDI; версии закреплены в [Dockerfile](../packaging/docker/Dockerfile.cuda).

## Первый запуск Docker

Базовый Compose настроен на NVIDIA CDI и без него не запустит даже служебный контейнер. До команд ниже выберите доступ к устройству: для CPU удалите секцию `devices` из своей копии Compose и задайте `models.device = "cpu"` в server.toml. Для дискретной AMD/Intel замените NVIDIA mapping на `/dev/dri:/dev/dri` и добавьте через `group_add` числовой GID render-группы хоста. Эти GPU требуют отдельной аппаратной проверки. Для NVIDIA сохраните mapping и заранее проверьте доступность CDI-устройства.

На подготовленном Linux-хосте из каталога исходников:

```sh
cd packaging/docker
cp server.profile-vulkan.toml server.toml
install -d -m 700 credentials
sudo chown 10001:10001 credentials
export FONO_IMAGE=ghcr.io/whitewolf06/fono-server:0.9.1
docker compose -f compose.example.yml config --quiet
docker compose -f compose.example.yml pull
docker compose -f compose.example.yml run --rm --no-build fono config check
docker compose -f compose.example.yml run --rm --no-build fono models pull whisper:large_turbo
docker compose -f compose.example.yml run --rm --no-build fono token create panel-admin --scope admin
docker compose -f compose.example.yml run --rm --no-build fono doctor
docker compose -f compose.example.yml up -d --no-build
curl --fail http://127.0.0.1:17832/healthz
```

Секрет admin-токена выводится один раз. Сохраните его отдельно; не помещайте в Git, образ или server.toml. Контейнер работает с UID/GID 10001:10001. Каталог credentials должен разрешать ему запись, файл токенов — принадлежать ему и иметь 0600. Монтируется каталог, чтобы атомарная замена была видна процессу. Хранятся salted hashes.

Модельный volume и fono-cache сохраняют веса и shader cache. Корневая файловая система read-only, capabilities сброшены. Команда down сохраняет именованные volumes; не добавляйте --volumes, если данные нужны.

Для CUDA задайте FONO_IMAGE=ghcr.io/whitewolf06/fono-server:0.9.1-cuda, используйте server.profile-a.toml и добавляйте -f compose.cuda.example.yml к каждому Compose-вызову. Исходные сборки и Docker-архивы описаны в [поставке](delivery.md).

## Конфигурация и защищённый доступ

Полный [образец TOML](../packaging/server.toml.example) описывает listener, модели, очередь и ограничения. CLI: fono-server --config /path/server.toml config check. Окружение переопределяет поддержанные поля; неверное значение указывает на поле.

По умолчанию сервер слушает loopback. В контейнере listener доступен Docker-сети, а [Compose](../packaging/docker/compose.example.yml) публикует порт хоста только на 127.0.0.1. Другой интерфейс требует явных trusted_private_network = true, allowed_hosts и защищённой сети. HTTP в обычной LAN не является шифрованным транспортом; между машинами используйте HTTPS или туннель.

TLS не заменяет bearer, Host allowlist, ограничения размера/времени и контроль сети. Не выставляйте API в Интернет только из-за наличия сертификата. CORS не разрешает произвольные origin.

## Панель и потребители

Войдите admin-токеном и откройте «Приложения». Задайте уникальное имя: 1–64 ASCII-символа из букв, цифр, "_" и "-", начиная с буквы или цифры. Укажите origin, доступный потребителю, без /v1 и пути маршрута. Создайте ключ и скопируйте .env до закрытия раздела:

```dotenv
FONO_HOST=http://fono-server:17832
FONO_TOKEN=<application-key>
```

Потребитель должен поддерживать эти переменные или сопоставить их своей конфигурации. Панель не меняет другое приложение и не создаёт сетевой маршрут. В контейнере 127.0.0.1 означает этот контейнер: нужен явно общий защищённый Docker network или иной достижимый адрес. SSH-forward Windows не является адресом серверного потребителя.

server.client_url или FONO_SERVER_CLIENT_URL задаёт origin для примера в панели. Это не listener, proxy или firewall; hostname должен входить в allowed_hosts. По входящему Host адрес не угадывается.

Панель выдаёт recognize-ключи, не создаёт и не отзывает admin-ключи. Утерянный секрет отозвать и создать заново. Отзыв запрещает следующую авторизацию, но не отменяет уже принятую работу. HTTP-операции управления описаны в [API](api.md).

## TLS и обновление сертификатов

На Ubuntu native rustls TLS включается настройкой:

```toml
[tls]
cert = "/etc/fono/tls/cert.pem"
key = "/etc/fono/tls/key.pem"
```

Сертификат содержит leaf и intermediate chain. Незашифрованный PEM-ключ должен соответствовать ему; неверные, просроченные и небезопасные файлы отклоняются. Обычно ключ имеет права 0600. Клиент должен доверять CA и проверять hostname; отключать TLS-проверку нельзя. На Windows native TLS сервера не реализован.

Для Compose добавьте [TLS override](../packaging/docker/compose.tls.example.yml). Монтируйте каталог сертификатов, а не отдельные файлы, чтобы замена через rename была видна. FONO_TLS_SERVER_NAME задаёт имя проверки healthcheck; FONO_TLS_CA_FILE — публичный CA частной PKI.

После установки согласованной пары cert/key отправьте сигнал:

```sh
docker compose -f compose.example.yml -f compose.tls.example.yml kill -s HUP fono
```

Неверное обновление сохраняет прежнюю конфигурацию. SIGHUP не сбрасывает quotas. Выпуск/продление сертификатов выполняет оператор, встроенного ACME нет. Без tls используется HTTP; с TLS plaintext fallback отсутствует.

## Модели, doctor и наблюдаемость

CLI предоставляет models list/pull/verify/remove. Артефакты проверяются по SHA256 и устанавливаются атомарно; прерванная загрузка не считается установленной. Настройте default, preload и max_loaded. Административные load/unload по API выключены по умолчанию и не скачивают веса.

healthz возвращает 503 при запуске, ошибке или остановке и 200 после готовности default-модели. Доступный процесс или handshake не подтверждает загруженную GPU-модель. Загрузка включает прогрев, shader cache нужно сохранять.

doctor проверяет конфигурацию, модельные хеши, права, каталоги, порт, сеть, TLS, runtime, handshake и ограниченную речевую пробу. Запускайте до сервиса тем же пользователем. --skip-self-test явно пропускает речевую пробу. Подсказки не раскрывают секреты; параметры и таймауты описаны в CLI-справке.

Логи и метрики ограничены техническими полями; аудио, текст, glossary и токены в них не включаются. /metrics требует admin и metrics.enabled = true; по умолчанию выключен. Неизвестная свободная GPU-память не подменяется нулём.

Лимиты по проверенному credential применяются до чтения тела. 429 rate_limited отличается от 429 busy очереди; уважайте Retry-After. Интерактивная работа получает приоритет на границе batch-окна. Уровни автоматической и ручной проверки описаны в [testing](testing.md).
