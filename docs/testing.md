# Ручное тестирование Fono

## Автоматический gate перед ручной проверкой

Из `src-tauri`:

```powershell
cargo fmt --all --check
cargo test --workspace --target-dir target-codex-review
cargo clippy --workspace --all-targets --target-dir target-codex-review -- -D warnings
cargo check -p fono --no-default-features --target-dir target-codex-review
cargo check -p fono --no-default-features --features whisper-wake --target-dir target-codex-review
cargo check -p fono --no-default-features --features sherpa-wake --target-dir target-codex-review
cargo check -p fono --target-dir target-codex-review
```

Supply-chain gate описан в `dependency-policy.md`. Автоматические проверки не
заменяют desktop smoke, soak/leak measurement и installer smoke.

Release manifest path автоматически проверяется командой:

```powershell
cd src-tauri
cargo check -p fono --release --target-dir target-codex-review
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
Worker fixture отдельно проверяет protocol-v2 health `ping → pong`; health API
возвращает `busy`, а не ожидает активную транскрипцию.
Отдельный fixture с зависшим worker подтверждает, что cancellation прерывает
ожидание менее чем за две секунды, завершает process/session и возвращает
`Cancelled`, а не ждёт штатного request deadline.

Для воспроизводимого baseline STT transport:

```powershell
cargo test -p fono stt::worker::tests::base64_transport_measurement_for_typical_recording_lengths --target-dir target-codex-review -- --nocapture
```

Последний замер 2026-08-11: 5/30/120 сек дали соответственно 213 511 B,
1 280 175 B и 5 120 175 B JSON; encode занял 3/24/102 ms. Время зависит от
машины, размер — contract test. Пяти­минутный буфер также проверяется на
помещение в `MAX_REQUEST_FRAME_BYTES` (16 MiB).

## Перед началом

- Перед переключением между dev и installer полностью завершите Fono через tray
  и убедитесь в Диспетчере задач, что `fono.exe` не остался запущенным.
- Для dev запускайте точный путь `src-tauri\target\release\fono.exe`, а не
  ярлык или список недавних приложений. Сверьте версию рядом с заголовком Fono.
- Используйте сборку из `src-tauri\target\release\fono.exe` или installer.
- Выберите микрофон и скачайте/выберите Whisper model.
- Если тестируете wake word, включите его отдельно: hotkey должен работать и
  при выключенном wake word.

## STT / acceleration

Для каждого режима **CUDA**, **Vulkan**, **Auto**, **CPU**:

1. Сохранить режим в настройках.
2. Нажать тест записи/распознавания и произнести короткую фразу.
3. Проверить текст, время и device в результате.
4. В `%APPDATA%\Fono\logs` проверить `STT backend selected`.

Ожидание: Auto на NVIDIA выбирает CUDA; при явном CUDA/Vulkan приложение не
должно тихо перейти на CPU.

## Wake word

- В Sherpa разрешены только `hey fono` и `okay fun`; сохранение другой фразы
  должно вернуть validation error. В Whisper Experimental произвольная phrase
  допускается, однако matching требует соседние слова в исходном порядке.

### Whisper Experimental

1. Указать фразу, например `okay fun`.
2. Использовать «Записать» → «Распознать запись».
3. Проверить live wake → post-wake диктовку → возврат listener в Listening.

### Sherpa-ONNX

1. Скачать KWS model в UI.
2. Кнопка `Эталон Sherpa WAV` должна обнаружить `LIGHT UP`.
3. Для ручного теста используйте `hey fono` или `okay fun`.
4. `Модель услышала: —` означает отсутствие keyword match, а не текстовую
   транскрипцию. Для произвольной фразы выберите Whisper Experimental.
5. Для диагностической dev-сборки проверьте в `%APPDATA%\Fono\logs` строку
   `creating keyword spotter`: она фиксирует фактическую фразу, BPE-граф,
   threshold и score, с которыми был создан Sherpa.

## Регрессии

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
