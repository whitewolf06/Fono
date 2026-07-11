# Ручное тестирование Fono

## Перед началом

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

## Регрессии

- Worker-процессы не открывают окно Terminal.
- Cancel не вставляет поздний результат.
- Clipboard mode не перезаписывает содержимое, скопированное пользователем во
  время диктовки.
- Hotkey, wake word и command-hotkey не конкурируют за активную запись.
- LM Studio failure возвращает raw transcript и понятную ошибку, не ломая UI.

## Что приложить к баг-репорту

- screenshot ошибки и выбранный backend;
- последние строки `%APPDATA%\Fono\logs`;
- Windows version, GPU/driver и содержимое settings без API key.
