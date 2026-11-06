/** Existing Russian UI wording. Keep identifiers stable for the future locale port. */
export const ruMessages = {
  "remoteHistory.backend": "Сервер Fono",
  "remoteHistory.server": "Сервер",
  "remoteHistory.device": "Устройство сервера",
  "remoteHistory.acceleration": "Ускорение сервера",
  "remoteHistory.decode": "Распознавание на сервере",
  "remoteHistory.connection": "Подготовка подключения",
  "remoteHistory.request": "Запрос к серверу",
  "remoteHistory.serverTotal": "На сервере, всего",
  "remoteHistory.network": "Передача по сети",
  "remoteHistory.timingNote":
    "Время сервера входит во время запроса. Отдельные этапы показаны, когда есть измерения.",
  "pretranscription.label": "Распознавать фразы во время записи",
  "pretranscription.saving": "Сохраняем настройку…",
  "pretranscription.ready": "Текст готов: выберите действие с диктовкой.",
  "pretranscription.hint":
    "После 15 секунд Fono начнёт распознавать законченные фразы в фоне. Итоговый текст появится после остановки записи. Пока включается вручную: качество длинной диктовки ещё проверяется.",
  "pretranscription.demo":
    "Демонстрационный режим: фоновое распознавание не запускается.",
  "pretranscription.unknown": "Получаем состояние распознавания…",
  "pretranscription.unavailable":
    "В этой сборке предварительное распознавание недоступно.",
  "pretranscription.off": "Выключено: запись распознаётся после остановки.",
  "pretranscription.idle": "Включено для следующей длинной диктовки.",
  "pretranscription.inactive":
    "В этой записи предварительное распознавание не запущено.",
  "pretranscription.waiting": "Ожидаем {seconds} секунд записи.",
  "pretranscription.listening":
    "Ждём паузу в речи. Подготовлено фрагментов: {count}.",
  "pretranscription.queued":
    "Фрагмент ожидает распознавания. Подготовлено: {count}.",
  "pretranscription.recognizing": "Распознаём фрагмент. Подготовлено: {count}.",
  "pretranscription.fallback":
    "Предварительное распознавание недоступно. Запись будет распознана после остановки.",
  "pretranscription.finishing": "Запись остановлена: готовим итоговый текст.",
  "common.details": "Подробнее",
  "common.retry": "Попробовать снова",
  "common.cancel": "Отмена",
  "common.close": "Закрыть",
  "common.copy": "Копировать",
  "common.copyText": "Копировать текст",
  "common.actionFailed": "Не удалось выполнить действие.",
  "load.loading": "Загружаем данные",
  "load.preparing": "Подготавливаем раздел…",
  "load.scenarios": "К сценариям проверки",
  "load.failed": "Не удалось загрузить данные",
  "load.demoError":
    "Демонстрационная ошибка соединения. Настройки и текущий текст сохранены в памяти сессии.",
  "health.ready": "Готово",
  "health.off": "Выключено",
  "health.missing": "Нужна настройка",
  "health.loading": "Загрузка",
  "health.error": "Ошибка",
  "phase.idle": "Готов слушать",
  "phase.listening": "Слушаю вас",
  "phase.silence": "Жду продолжения",
  "phase.transcribing": "Распознаю речь",
  "phase.processing": "Привожу текст в порядок",
  "phase.awaiting_action": "Выберите действие с текстом",
  "phase.done": "Текст готов",
  "phase.cancelled": "Запись отменена",
  "phase.error": "Нужна ваша помощь",
  "dictation.noSpeech": "Речь не обнаружена. Попробуйте записать ещё раз.",
  "decoding.mode": "Режим распознавания",
  "decoding.fast": "Быстро",
  "decoding.accurate": "Точно",
  "decoding.hint":
    "Точный режим использует более широкий поиск и может работать дольше. Качество зависит от записи.",
  "decoding.threads": "Потоки процессора",
  "decoding.autoThreads": "Автоматически · до 8 потоков",
  "decoding.threadsHint":
    "Автоматический выбор сохраняет прежнюю настройку. Меньше потоков оставляет больше ресурсов другим приложениям.",
  "decoding.invalidMode": "Выберите режим распознавания: быстро или точно.",
  "decoding.invalidThreads":
    "Выберите от 1 до 8 потоков или автоматический режим.",
  "pending.processing": "Обрабатываю текст",
  "pending.ready": "Текст готов",
  "pending.choose": "Выберите действие с текстом",
  "pending.actions": "Действия с диктовкой",
  "pending.keepOriginal": "Оставить исходный текст в Fono",
  "pending.insertOriginal": "Вставить исходный текст без обработки",
  "pending.original": "Исходный",
  "pending.working": "Обработка…",
  "pending.processLanguage": "Обработать · {language}",
  "pending.process": "Обработать",
  "pending.copyClose": "Копировать текст и закрыть индикатор",
  "pending.closeWithoutCopy": "Закрыть без копирования",
  "pending.cancel": "Отменить диктовку",
  "processing.styleLabel": "Стиль обработки",
  "processing.style": "Стиль",
  "processing.formal": "Деловой",
  "processing.translationLabel": "Перевод после обработки",
  "processing.translation": "Перевод",
  "processing.none": "Нет",
  "processing.saving": "Сохраняю выбор…",
  "processing.remembered": "Выбор сохраняется для следующих диктовок",
  "notice.copyTitle": "Не удалось скопировать",
  "notice.copyHint":
    "Текст остаётся доступным. Попробуйте скопировать ещё раз.",
  "notice.insertionTitle": "Вставка остановлена",
  "notice.insertionHint":
    "Скопируйте текст. Перед вставкой проверьте поле: часть текста могла уже вставиться.",
  "notice.processingTitle": "Не удалось обработать текст",
  "notice.processingHint":
    "Исходный текст доступен для копирования или повторной обработки.",
  "notice.otherTitle": "Не удалось завершить действие",
  "notice.hasTextHint":
    "Текст доступен для копирования. Причина — в подробностях.",
  "notice.otherHint": "Посмотрите причину в подробностях и попробуйте ещё раз.",
  "recovery.recognizing": "Распознаю сохранённую запись…",
  "recovery.chooseSource": "Где распознать запись?",
  "recovery.serverFailed": "Сервер не распознал запись",
  "recovery.serverUnavailable": "Сервер недоступен",
  "recovery.localFailed": "Локальное распознавание не удалось",
  "recovery.retained":
    "Запись остаётся в памяти Fono до результата или отмены. Выберите, что сделать дальше.",
  "recovery.localMissing":
    "Локальная модель не настроена или не установлена. Можно повторить запрос к серверу.",
  "recovery.retry": "Повторить на сервере",
  "recovery.local": "Распознать локально",
  "recovery.cancel": "Отменить диктовку",
  "recovery.actions": "Действия с сохранённой записью",
  "recovery.historyTitle": "Источник распознавания",
  "recovery.historyFallback": "Резерв · локально на CPU",
  "remoteCheck.localFallback": "Сервер не готов · попробуем резерв на CPU",
  "remoteCheck.askFallback": "Сервер не готов · после записи будет выбор",
  "recognitionSource.title": "Где распознавать речь",
  "recognitionSource.local": "Этот компьютер",
  "recognitionSource.server": "Сервер Fono",
  "recognitionSource.name": "Имя сервера",
  "recognitionSource.namePlaceholder": "Например, Kari",
  "recognitionSource.url": "Адрес сервера",
  "recognitionSource.urlHint":
    "HTTPS или HTTP через локальный SSH-туннель. Для Kari: http://127.0.0.1:17833. HTTP по сети недоступен.",
  "recognitionSource.privacy":
    "Аудио будет отправляться на сервер «{server}» после сохранения этих настроек.",
  "recognitionSource.unnamed": "Fono",
  "recognitionSource.demo":
    "Это браузерный пример: данные сервера демонстрационные, сетевых запросов и сохранения токена нет.",
  "recognitionSource.token": "Токен доступа",
  "recognitionSource.tokenHint":
    "Проверка не сохраняет токен. Сохраните токен отдельно, затем настройки подключения. Сохранённый токен не показывается.",
  "recognitionSource.saveToken": "Сохранить токен",
  "recognitionSource.deleteToken": "Удалить токен",
  "recognitionSource.tokenStored": "Токен сохранён",
  "recognitionSource.tokenMissing": "Токен не сохранён",
  "recognitionSource.tokenUnknown": "Проверяем наличие токена…",
  "recognitionSource.tokenSaved":
    "Токен сохранён. Адрес, источник и модель применяются кнопкой «Сохранить» в настройках.",
  "recognitionSource.tokenDeleted":
    "Токен удалён. Для подключения понадобится новый.",
  "recognitionSource.tokenStatusFailed":
    "Не удалось проверить наличие токена. Повторите открытие настроек.",
  "recognitionSource.tokenSaveFailed":
    "Не удалось сохранить токен. Завершите активные операции и попробуйте ещё раз.",
  "recognitionSource.tokenDeleteFailed":
    "Не удалось удалить токен. Завершите активные операции и попробуйте ещё раз.",
  "recognitionSource.ca": "Сертификат собственного центра доверия",
  "recognitionSource.caHint":
    "Необязательно: публичный CA-сертификат в формате PEM. Проверка HTTPS остаётся включённой.",
  "recognitionSource.check": "Проверить подключение",
  "recognitionSource.checking": "Проверяем сервер…",
  "recognitionSource.checked": "Сервер ответил",
  "recognitionSource.observation":
    "Результат последней проверки черновика. Настройки ещё нужно сохранить.",
  "recognitionSource.state": "Состояние",
  "recognitionSource.state.ready": "Готов",
  "recognitionSource.state.starting": "Запускается",
  "recognitionSource.state.busy": "Занят",
  "recognitionSource.state.failed": "Ошибка сервера",
  "recognitionSource.state.stopping": "Останавливается",
  "recognitionSource.deviceUnavailable": "недоступно",
  "recognitionSource.model": "Модель на сервере",
  "recognitionSource.modelHint":
    "Проверьте подключение, чтобы выбрать установленную модель сервера.",
  "recognitionSource.fallback": "Если сервер недоступен",
  "recognitionSource.fallbackNone": "Сохранить запись и предложить повтор",
  "recognitionSource.fallbackAsk": "Спросить перед отправкой записи",
  "recognitionSource.fallbackLocal": "Использовать локальный резерв",
  "recognitionSource.fallbackModel": "Резервная модель на этом компьютере",
  "recognitionSource.fallbackHint":
    "Используется только установленная модель, на CPU. Она загрузится при обращении к резерву.",
  "recognitionSource.manualFallback":
    "Для ручного локального повтора используется сохранённая модель этого компьютера.",
  "recognitionSource.limits":
    "Подключается только Fono Server. Аудио передаётся без сжатия.",
  "recognitionSource.locked":
    "Источник и параметры подключения меняются между диктовками. Завершите текущую операцию.",
  "recognitionSource.gpuUnused":
    "Локальная видеокарта не используется для распознавания на сервере. Резерв работает на CPU.",
  "recognitionSource.serverModels": "Модели сервера",
  "recognitionSource.serverModelsHint":
    "Список только для просмотра. Установка и удаление моделей выполняются на сервере.",
  "recognitionSource.installed": "Установлена на сервере",
  "recognitionSource.notInstalled": "Не установлена",
  "recognitionSource.invalidEndpoint":
    "Проверьте адрес: нужен HTTPS либо HTTP на 127.0.0.1 или localhost через SSH-туннель, без пути и параметров.",
  "recognitionSource.notConfigured":
    "Для изменённого подключения введите токен и проверьте его. Сохранённый токен используется только для сохранённого подключения.",
  "recognitionSource.network":
    "Сервер недоступен. Проверьте адрес, сеть и SSH-туннель.",
  "recognitionSource.timeout": "Сервер не ответил вовремя. Попробуйте ещё раз.",
  "recognitionSource.unauthorized":
    "Сервер не принял токен. Проверьте его и повторите подключение.",
  "recognitionSource.incompatible":
    "Эта версия Fono Server несовместима с приложением. Обновите сервер.",
  "recognitionSource.unsupportedCapabilities":
    "Сервер не поддерживает нужные возможности распознавания. Обновите Fono Server.",
  "recognitionSource.modelMissing":
    "Выбранная модель не установлена на сервере. Выберите другую из полученного списка.",
  "recognitionSource.invalidResponse":
    "Сервер вернул неподдерживаемый ответ. Проверьте адрес и версию Fono Server.",
  "recognitionSource.busy":
    "Сервер занят. Попробуйте проверить подключение позже.",
  "recognitionSource.serverFailed":
    "На сервере ошибка. Проверьте его состояние в веб-панели.",
  "recognitionSource.invalidSettings":
    "Проверьте адрес, модель и параметры резервного распознавания.",
  "recognitionSource.invalidToken":
    "Введите токен без пробелов, до 4096 символов.",
  "recognitionSource.transport": "Как подключаться",
  "recognitionSource.directTransport": "По адресу сервера",
  "recognitionSource.sshTransport": "Через SSH · подключается Fono",
  "recognitionSource.sshHint":
    "Сохраните подключение один раз. Fono сам откроет защищённый канал при обращении к серверу. Нужен OpenSSH Client в Windows.",
  "recognitionSource.sshHost": "Адрес SSH-сервера",
  "recognitionSource.sshUser": "Пользователь SSH",
  "recognitionSource.sshIdentity": "Файл личного SSH-ключа",
  "recognitionSource.sshIdentityHint":
    "Выберите существующий ключ. Для ключа с паролем нужен разблокированный SSH-агент Windows. Если поле пустое, используется ключ агента.",
  "recognitionSource.sshChooseIdentity": "Выбрать файл ключа",
  "recognitionSource.sshPorts": "Порты подключения",
  "recognitionSource.sshPort": "Порт SSH",
  "recognitionSource.sshServerPort": "Порт Fono Server",
  "recognitionSource.sshServerPortHint":
    "Порт API на самом сервере. Обычно 17832.",
  "recognitionSource.sshProbeHost": "Получить ключ сервера",
  "recognitionSource.sshFingerprint": "Отпечаток ключа сервера",
  "recognitionSource.sshTrustHint":
    "Сверьте отпечаток с данными владельца сервера. Подтверждайте только знакомый ключ: по нему Fono проверяет, к какому серверу подключается.",
  "recognitionSource.sshTrustHost": "Доверять этому ключу",
  "recognitionSource.sshTrusted": "Ключ сервера выбран. Сохраните настройки.",
  "recognitionSource.sshInvalidConfiguration":
    "Проверьте адрес, пользователя, порты и файл SSH-ключа.",
  "recognitionSource.sshClientMissing":
    "В Windows не найден OpenSSH Client. Установите этот дополнительный компонент Windows и повторите подключение.",
  "recognitionSource.sshIdentityMissing":
    "Файл SSH-ключа недоступен. Выберите существующий файл.",
  "recognitionSource.sshHostKeyRequired":
    "Получите ключ сервера, сверьте отпечаток и подтвердите доверие.",
  "recognitionSource.sshConnectionFailed":
    "SSH-соединение не открылось. Проверьте сеть, пользователя, ключ и доступ к серверу. Изменившийся ключ сервера нужно проверить заново.",
  "recognitionSource.sshProbeFailed":
    "Не удалось получить ключ сервера. Проверьте адрес, порт SSH и сеть. Личный ключ и токен на этом шаге не используются.",
  "recognitionSource.sshBusy":
    "SSH-подключение занято. Завершите текущую операцию и попробуйте ещё раз.",
} as const;
