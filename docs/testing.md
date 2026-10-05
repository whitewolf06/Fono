# Ручное тестирование Fono

Текущее приложение использует классическую диктовку с режимами горячей клавиши
`hold` и `toggle`. Основная ручная приёмка — запись, отмена, вставка, принятие
в overlay без вставки, копирование и выбор обработки/перевода во время записи.
WakeWord временно недоступен; поэтапный вывод «На лету» скрыт.

Актуальные сценарии:

- [STATUS.md](STATUS.md) — доступные возможности и границы подтверждения.
- [fono-v3-qa.md](fono-v3-qa.md) — интерфейс и состояния V3.
- [overlay-prompts-qa.md](overlay-prompts-qa.md) — overlay, промпты и обработка.
- [fono-memory-updates-qa.md](fono-memory-updates-qa.md) — память модели и updater.
- [fono-voice-reliability.md](fono-voice-reliability.md) — голосовое ядро,
  сохранённые экспериментальные пути и результаты ограниченных замеров.

Голос, глобальные клавиши и пользовательские окна проверяет пользователь.
Ниже сохранены общие и исторические методики qualification; проверки WakeWord
и живого вывода не являются инструкцией включить их в текущей версии.

## Автоматический gate перед ручной проверкой

Из корня репозитория в **PowerShell 7**, с установленными build-зависимостями:

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
} finally {
    Pop-Location
}
```

Wrapper сам меняет рабочий каталог на `src-tauri`, собирает точный набор
тестов и размещает четыре закреплённые Sherpa/ONNX DLL рядом с executable.
Прямой `cargo test` с одним `PATH` на Windows может загрузить несовместимый
системный ONNX runtime. Подробности: [fono-ci-updates.md](fono-ci-updates.md).

Supply-chain gate описан в [dependency-policy.md](dependency-policy.md). Автоматические проверки не
заменяют desktop smoke, soak/leak measurement и installer smoke.

Release manifest path автоматически проверяется командой:

```powershell
cd src-tauri
cargo check --locked -p fono --release --target-dir target-codex-review
```

Она подтверждает, что `build.rs` принимает закрытый набор STT worker/Sherpa
resources, но не собирает installer и не проверяет установку.

Unit-тесты `fono-core` проверяют, что lease принадлежит только активной
операции, duplicate/stale acquisition отклоняется, а terminal failure очищает
все leases. Отдельный детерминированный цикл из 1000 `start → leases →
cancel/failed` подтверждает, что после каждого terminal-состояния не остаётся
активной операции или lease.

Локальный HTTP fault-injection harness для `LlmClient` не использует сеть и
проверяет реальный transport path: HTTP 503, malformed JSON и ответ больше
2 MiB превращаются в контролируемую ошибку.

Overlay adapter fault-injection проверяет, что отказ `show/hide` фиксируется в
логах и не пробрасывается в operation lifecycle; terminal transition не зависит
от доступности окна.

`SttEngine` unit-тест проверяет начальное `unloaded` readiness и переход в
`failed` при недоступной модели — без запуска Whisper, worker или микрофона.
Фоновый preload использует тот же `ensure_loaded` и load-gate; его реальная
проверка с выбранной моделью относится к desktop dev/manual уровню, потому что
создание Tauri `AppHandle` и запуск native Whisper не являются unit-test средой.
Worker fixture отдельно проверяет protocol-v3 health `ping → pong`; health API
возвращает `busy`, а не ожидает активную транскрипцию.
Отдельный fixture с зависшим worker подтверждает, что cancellation прерывает
ожидание менее чем за две секунды, завершает process/session и возвращает
`Cancelled`, а не ждёт штатного request deadline.
Ещё один Windows fixture 100 раз запускает worker с malformed response и
проверяет после каждого отказа kill/wait cleanup и возможность следующего
handshake; это regression gate для restart path без GPU и реальной модели.
Mailbox fixture проверяет owner-thread `ping` и отмену зависшей транскрипции:
она возвращает `Cancelled`, завершает owner session и не удерживает health за
длительным inference.

Для воспроизводимого baseline STT transport (из корня после bootstrap):

```powershell
./scripts/test-native.ps1 -CargoArguments @('--locked', '-p', 'fono', 'stt::worker::tests::base64_transport_measurement_for_typical_recording_lengths', '--target-dir', 'target-codex-review') -TestArguments @('--nocapture')
```

Последний замер 2026-08-11: 5/30/120 сек дали соответственно 213 511 B,
1 280 175 B и 5 120 175 B JSON; encode занял 3/24/102 ms. Время зависит от
машины, размер — contract test. Пяти­минутный буфер также проверяется на
помещение в `MAX_REQUEST_FRAME_BYTES` (16 MiB).

## Сохранённые методики qualification

Общие STT, отмена и вставка применимы к текущей классической диктовке. Сведения
о старых wake backends, прежних UI-кнопках и замерах ниже сохранены для
регрессий и будущей доработки. Проверки wake-пути выполнять только после
отдельного восстановления функции и согласования её качества.

### Перед началом

- Перед переключением между dev и installer полностью завершите Fono через tray
  и убедитесь в Диспетчере задач, что `fono.exe` не остался запущенным.
- Для dev запускайте точный путь `src-tauri\target\release\fono.exe`, а не
  ярлык или список недавних приложений. Сверьте версию рядом с заголовком Fono.
- Используйте сборку из `src-tauri\target\release\fono.exe` или installer.
- Выберите микрофон и скачайте/выберите Whisper model.
- WakeWord в текущем приложении недоступен. После его восстановления
  hotkey должен работать независимо от включения пробуждения.

## STT / acceleration

Для каждого режима **CUDA**, **Vulkan**, **Auto**, **CPU**:

1. Сохранить режим в настройках.
2. Нажать тест записи/распознавания и произнести короткую фразу.
3. Проверить текст, время и device в результате.
4. В `%APPDATA%\Fono\logs` проверить `STT backend selected`.

Ожидание: Auto на NVIDIA выбирает CUDA; при явном CUDA/Vulkan приложение не
должно тихо перейти на CPU.

## Диагностика окончания диктовки

Проверка выполняется в **Desktop dev** или собранном Tauri-приложении: браузерный
`npm run dev:ui` не запускает реальный микрофон, hotkey и wake pipeline.

1. Для global hotkey прогоните короткую обычную фразу; историческую проверку
   wake-пути повторите после восстановления функции:
   с тихим окончанием, с короткой паузой и без паузы. Повторите с выключенной и
   включённой AI-обработкой.
2. После каждого прогона найдите в `%APPDATA%\Fono\logs` единственную строку
   `event="dictation_tail_diagnostic"` с тем же `operation`.
3. Сравните `source`, `stop_reason`, `captured_ms`, VAD-поля, `stt`,
   `postprocessor` и `first_suspected_layer`. Значения `capture`, `vad`, `stt`
   и `postprocessor` указывают первый технически подозрительный слой; `none`
   означает, что по числовым измерениям причина не обнаружена.
4. В wake-пути ожидается `vad_applied=false`: он намеренно не делает второй
   offline trim после realtime-таймера тишины. В hotkey-пути VAD применяется
   только к ведущей тишине: при найденной речи `vad_trailing_after_ms` должен
   быть равен `vad_trailing_before_ms`. Это подтверждает, что хвост уже
   записанной фразы полностью передан в STT.

В записи нет аудиосэмплов, фрагментов транскрипта, пути к модели или текста
ошибок. Логи содержат только длительности, счётчики и статусы этапов.

## Wake word — историческая методика

Этот раздел описывает прежние backend-ы и их проверки. Native gate текущего
приложения запрещает включение, запись теста и калибровку WakeWord. Для будущей
потоковой RU/EN qualification используйте [fono-voice-reliability.md](fono-voice-reliability.md).

- В Sherpa разрешены только `hey fono`, `okay fun` и `рамзи`; сохранение другой
  фразы должно вернуть validation error. В Whisper Experimental произвольная
  phrase допускается, однако matching требует соседние слова в исходном порядке.

### Whisper Experimental

1. Указать фразу, например `okay fun`.
2. Использовать «Записать» → «Распознать запись».
3. Проверить live wake → post-wake диктовку → возврат listener в Listening.

### Sherpa-ONNX

1. Скачать KWS model в UI.
2. Кнопка `Эталон Sherpa WAV` должна обнаружить `LIGHT UP`.
3. Для ручного теста используйте `hey fono`, `okay fun` или `рамзи`.
4. `Модель услышала: —` означает отсутствие keyword match, а не текстовую
   транскрипцию. Для произвольной фразы выберите Whisper Experimental.
5. Для диагностической dev-сборки проверьте в `%APPDATA%\Fono\logs` строку
   `creating keyword spotter`: она фиксирует фактическую фразу, BPE-граф,
   threshold и score, с которыми был создан Sherpa.

## Регрессии

Общие сценарии остаются применимыми. Пункты с WakeWord относятся к будущей
qualification после возврата функции; сейчас их не требуется включать в UI.

- Local REST: с выбранной Whisper-моделью выполнить health, отправить WAV через
  `/v1/transcriptions`, дождаться `completed`, затем проверить stop приложения
  и освобождение loopback-порта. Детальный сценарий —
  [LOCAL_TRANSCRIPTION_API.md](LOCAL_TRANSCRIPTION_API.md).

- Wake → post-wake dictation использует один `AudioHub`: устройство не должно
  переоткрываться, а после возврата listener продолжает получать аудио.
- Unit-test с инъецируемым audio adapter проверяет: отказ получения устройства
  завершает operation и не оставляет resource lease без реального микрофона.
- Голосовая command-hotkey и явная команда после wake phrase создают preview;
  confirm после 30 секунд или после изменения settings должен быть отклонён и не
  выполнять действие.
- Worker-процессы не открывают окно Terminal.
- Во время длительной STT-транскрипции смена runtime-state не должна зависать на
  глобальном маршрутизаторе STT; worker error должен очистить только текущую
  session и позволить следующей операции создать новую.
- Cancel не вставляет поздний результат; для standalone worker также
  останавливает зависший process, а не только отбрасывает результат.
- Clipboard mode не перезаписывает содержимое, скопированное пользователем во
  время диктовки.
- Clipboard mode восстанавливает прежний text/image/file-list и не вставляет в
  окно, если foreground HWND изменился перед Ctrl+V.
- Hotkey, wake word и command-hotkey не конкурируют за активную запись.
- LM Studio failure возвращает raw transcript и понятную ошибку, не ломая UI.

## Что приложить к баг-репорту

- screenshot ошибки и выбранный backend;
- последние строки `%APPDATA%\Fono\logs` (логи ротируются, хранится 14 файлов);
- Windows version, GPU/driver и содержимое settings без API key.
