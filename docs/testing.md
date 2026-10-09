# Проверки и ручная приёмка Fono

Fono использует классическую диктовку с горячей клавишей в режимах `hold` и `toggle`. WakeWord временно недоступен, поэтапный вывод «На лету» скрыт. Методики этих экспериментов находятся в [fono-voice-reliability.md](fono-voice-reliability.md) и не входят в текущую приёмку.

Проверяйте три уровня отдельно:

| Уровень               | Как запустить                                                              | Что подтверждает                                                      |
| --------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| UI в браузере         | `npm run dev:ui`, `http://127.0.0.1:1420/v3.html`                          | Vue, mock-контракты, состояния, компоновку и browser adapters         |
| Desktop без установки | `npm run dev:desktop:v3` или собранный `src-tauri/target/release/fono.exe` | Настоящие Rust/Tauri, микрофон, hotkey, индикатор и внешнюю вставку   |
| Поставляемая сборка   | `npm run release`, затем пользовательская установка                        | Ресурсы NSIS/MSI, установку, запуск и обновление установленной версии |

Агент не записывает микрофон и не управляет пользовательским приложением ради UAT. Окна, голос, поля, UAC и установку пользователь проверяет самостоятельно. Приёмка на чистой Windows остаётся отдельным отложенным этапом.

## Автоматические проверки

Из корня репозитория:

```powershell
npm run typecheck:v3
npm run lint
npm run test:v3
npm run test:release
npm run build
npm run version:check
npm run format:check
```

`test:ui` запускает тот же набор, что `test:v3`. Проверки форматирования охватывают Vue V3, конфиг ESLint, README, документацию и правила агентов. Результат конкретного прогона относится к проверенному commit и окружению.

### Rust и native runtime

В PowerShell 7 с установленными build-зависимостями, из корня:

```powershell
./scripts/bootstrap-ci-resources.ps1
./scripts/test-native.ps1 -CargoArguments @('--locked', '--workspace', '--all-targets', '--target-dir', 'target-codex-review')
Push-Location src-tauri
try {
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets --target-dir target-codex-review -- -D warnings
    cargo check --locked -p fono --no-default-features --target-dir target-codex-review
    cargo check --locked -p fono --no-default-features --features whisper-wake --target-dir target-codex-review
    cargo check --locked -p fono --no-default-features --features sherpa-wake --target-dir target-codex-review
    cargo check --locked -p fono --target-dir target-codex-review
    cargo deny --config deny.toml check
} finally {
    Pop-Location
}
```

Wrapper собирает выбранные тесты и размещает закреплённые Sherpa/ONNX DLL рядом с executable. Одного `PATH` на Windows недостаточно: системный ONNX runtime может оказаться несовместимым. Подробности — [CI и ресурсы](fono-ci-updates.md), политика лицензий и advisories — [dependency-policy.md](dependency-policy.md).

Тесты покрывают владение операциями и leases, stale/duplicate Stop и Cancel, ошибки LLM transport, отказ окна индикатора, readiness и protocol-3 worker fixtures. Они используют адаптеры и проверяют отдельные контракты; работа настоящего Whisper, микрофона и GPU требует desktop-проверки.

Размер JSON/base64 transport для типичных длительностей и предел кадра проверяются воспроизводимым тестом:

```powershell
./scripts/test-native.ps1 -CargoArguments @('--locked', '-p', 'fono', 'stt::worker::tests::base64_transport_measurement_for_typical_recording_lengths', '--target-dir', 'target-codex-review') -TestArguments @('--nocapture')
```

После `npm run prepare:release-resources` закрытый manifest ресурсов дополнительно проверяется командой `cargo check --locked -p fono --release` из `src-tauri`. Эта проверка не создаёт installer и не подтверждает установку.

## UI в браузере

На размерах 720×560, 900×720 и 1440×900 проверьте страницы из [карты маршрутов](frontend-architecture.md):

1. Отсутствие горизонтальной прокрутки, длинные названия моделей и большой последний текст.
2. Диалоги и селекты: Tab/Shift+Tab, Escape, возврат фокуса и положение страницы.
3. Общий черновик настроек, сохранение, отмену, защиту несохранённых правок и откат выключателя при ошибке.
4. Запись на главной, редактирование, улучшение, отмену и копирование отображаемого текста.
5. Историю, поиск, отсутствие исходной версии без согласия, тренер, словарь, команды и очередь API.
6. Первоначальную настройку, прямые маршруты, «Назад», обновления и состояния ожидания/ошибки.
7. Общий компонент индикатора через `#/overlay`: компактный/подробный вид, настройки, принятие и отмену.
8. Демосценарии через `#/scenarios`, сброс mock и отсутствие ошибок приложения в консоли.

Browser mock не подтверждает native IPC, глобальную клавишу, запись аудио, распознавание, сетевой API, VRAM или установку.

## Desktop: подготовка и сценарии

Перед переключением dev/installer полностью завершите Fono через tray. Для прямого запуска используйте точный путь собранного EXE и сверьте его версию. Выберите микрофон, установленную Whisper-модель и нужное ускорение. Изоляция debug-настроек через `FONO_TEST_DATA_DIR` и отдельный API-порт описаны в [development.md](development.md).

| Сценарий                                                                      | Инструкция                                                   |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------ |
| Удержание/повторное нажатие, отмена, ручной выбор, история и смена поля       | [dictation-controls-qa.md](dictation-controls-qa.md)         |
| Индикатор без перехвата фокуса, сохранение выбора, промпты и пробная диктовка | [overlay-prompts-qa.md](overlay-prompts-qa.md)               |
| Память Whisper, отключённый WakeWord и проверка обновлений                    | [fono-memory-updates-qa.md](fono-memory-updates-qa.md)       |
| Метаданные истории, локальный словарь и отчёт диагностики                     | [fono-quality-enhancements.md](fono-quality-enhancements.md) |
| Health, WAV upload/polling, очередь и отмена API                              | [LOCAL_TRANSCRIPTION_API.md](LOCAL_TRANSCRIPTION_API.md)     |
| Загрузчик, продолжение загрузки, подпись, UAC и NSIS                          | [online-installer.md](online-installer.md)                   |

Для каждого ускорения Auto/CUDA/Vulkan/CPU сверяйте выбранный режим и фактический backend. Явный CUDA/Vulkan при недоступности возвращает ошибку; Auto может выбрать следующий доступный backend. Отдельно нужны AMD/Intel и чистая Windows.

Проверьте тихое окончание фразы, короткую паузу, полезные короткие слова, тишину и случайный щелчок. Длительная обработка не должна блокировать UI. Cancel запрещает позднюю вставку; следующий запуск после отмены должен работать. При изменении исходного поля или частичной отправке результат остаётся для копирования, автоматическая повторная вставка блокируется. Worker-процессы не открывают Terminal.

### Диагностика окончания фразы

Для локальной диагностики после Stop найдите в `%APPDATA%\Fono\logs` запись `event="dictation_tail_diagnostic"` с тем же `operation`. Сравните `source`, `stop_reason`, `captured_ms`, VAD-поля, `stt`, `postprocessor` и `first_suspected_layer`. Значение `none` означает, что числовые измерения не обнаружили подозрительного этапа.

В hotkey-пути VAD удаляет ведущую тишину; при найденной речи `vad_trailing_after_ms` должен совпадать с `vad_trailing_before_ms`. Запись содержит длительности, счётчики и статусы, без аудио и фрагментов транскрипта. Это техническая подсказка, а не измерение точности распознавания.

## Что сохранить по итогам приёмки

Укажите версию/ревизию, уровень проверки, Windows/GPU/driver, выбранный и фактический backend, шаги воспроизведения, ожидаемый и полученный результат. Для отчёта используйте «Настройки → Диагностика»: разрешённые технические поля собираются без пользовательских текстов, ключей, путей и сырых логов. При необходимости добавьте проверенный пользователем screenshot.

Пройденные unit tests, сборка EXE, публикация release и ручная приёмка фиксируются отдельно. Числа старого прогона не переносятся на новую версию.
