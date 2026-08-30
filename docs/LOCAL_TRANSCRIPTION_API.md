# Локальный API транскрибации

Fono при запуске поднимает REST-сервис только на `127.0.0.1:17832`. Он не
доступен по сети. Значение порта можно задать до запуска приложения через
`FONO_API_PORT`; значение `0` и некорректные порты отклоняются.

Полное машиночитаемое описание находится в
[`src-tauri/openapi.json`](../src-tauri/openapi.json).

## Авторизация

При первом запуске Fono создаёт bearer-токен в `%APPDATA%\Fono\api-token.txt`.
Это локальный секрет: не добавляйте его в код, логи, issue или общие
конфигурации. Получить его для локального клиента можно так:

```powershell
$token = (Get-Content "$env:APPDATA\Fono\api-token.txt" -Raw).Trim()
```

Проверка health:

```powershell
Invoke-RestMethod http://127.0.0.1:17832/v1/health `
  -Headers @{ Authorization = "Bearer $token" }
```

## Загрузка файла

Основной маршрут — `POST /v1/transcriptions`, `multipart/form-data`.

- `audio` — обязательный файл WAV, MP3, FLAC, OGG/Vorbis или OGG/Opus (в том
  числе Telegram voice messages), до 100 MiB;
- `model` — обязательный публичный идентификатор модели, выбранной сейчас в Fono
  (`tiny`, `base`, `small`, `medium`, `large` или `large_turbo`);
- `language` — необязательный ISO-код или `auto` (значение по умолчанию).

Пример:

```powershell
curl.exe --fail http://127.0.0.1:17832/v1/transcriptions `
  -H "Authorization: Bearer $token" `
  -F "audio=@sample.wav" `
  -F "model=base" `
  -F "language=auto"
```

В ответ приходит job со статусом `queued`. Временная копия загрузки удаляется
после декодирования независимо от результата.

## Опрос и отмена

```powershell
$status = Invoke-RestMethod "http://127.0.0.1:17832/v1/transcription-jobs/$($job.id)" `
  -Headers @{ Authorization = "Bearer $token" }

Invoke-RestMethod "http://127.0.0.1:17832/v1/transcription-jobs/$($job.id)/cancel" `
  -Method Post `
  -Headers @{ Authorization = "Bearer $token" }
```

Состояния: `queued`, `preparing`, `transcribing`, `completed`, `failed`,
`cancelled`. Результат с текстом появляется в `result`, ошибка — в `error`.
Для диагностики доступны `created_at_ms`, `started_at_ms` и `finished_at_ms` —
Unix-время в миллисекундах.

## Спецификация API

Версионированная спецификация доступна локально по
`GET /openapi.json`. Как и остальные маршруты, она требует bearer-токен:

```powershell
Invoke-RestMethod "http://127.0.0.1:17832/openapi.json" `
  -Headers @{ Authorization = "Bearer $token" }
```
Значение `result.model` всегда соответствует модели, действительно выбранной
в Fono. Если `model` запроса с ней не совпадает, job завершается с
`invalid_request` до запуска инференса.
Во время интерактивной диктовки очередная service-job ждёт освобождения общей
STT-операции.

`POST /v1/transcription-jobs` оставлен как низкоуровневый JSON-вход для уже
нормализованного mono PCM 16 kHz; для файлов используйте multipart-маршрут.

## Ручной smoke-test

1. Запустите Fono desktop dev и выберите Whisper-модель в UI.
2. Выполните health-запрос и отправьте небольшой WAV-файл.
3. Опросите job до `completed`; текст должен быть в `result.text`.
4. Во время обработки попробуйте начать диктовку в UI: она не должна
   конкурировать с REST-задачей за STT.
5. Закройте Fono и убедитесь, что локальный порт больше не принимает запросы.

Эта проверка требует настоящей модели и Tauri runtime; unit-тесты её не заменяют.
