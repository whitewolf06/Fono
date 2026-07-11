# Fono

Локальный Windows voice layer: диктовка в активное окно, wake word, overlay,
локальная AI-обработка через LM Studio и безопасные голосовые команды.

## Текущий статус

Рабочий Windows MVP. Основной сценарий — hotkey или wake word → запись →
Whisper → опциональная обработка → вставка текста.

- Push-to-talk и post-wake диктовка с отменой, VAD и overlay.
- Модели Whisper загружаются через UI и хранятся в `%APPDATA%\Fono\whisper-models`.
- Режимы STT: **Auto**, **CUDA**, **Vulkan**, **CPU**.
  Auto выбирает CUDA → Vulkan → встроенный GPU → CPU.
- CUDA и Vulkan поставляются отдельными worker-процессами; их консоль не
  отображается пользователю.
- Wake word: `Whisper Experimental` для гибкой фразы, `Sherpa-ONNX` для
  быстрого KWS. Для Sherpa сейчас проверены `hey fono` и `okay fun`; произвольная
  фраза потребует корректной BPE-токенизации.
- LM Studio уже подключён для clean/format/command режимов.
- Базовые команды: громкость, медиа, запуск приложений из allowlist и фокус окон.

## Сборка

Нужны Rust, Node.js, MSVC Build Tools, CMake, Vulkan SDK и CUDA Toolkit на
**машине сборки**. Конечному пользователю CUDA Toolkit не нужен: нужные CUDA
runtime DLL поставляются с CUDA worker.

```powershell
npm install
npm run release
```

`npm run release` сначала собирает frontend и CUDA/Vulkan worker'ы, затем
создаёт NSIS/MSI через Tauri. Артефакты появляются в
`src-tauri\target\release\bundle\`.

Для быстрой разработки:

```powershell
npm run tauri dev
```

## Документы

- [Текущий статус](docs/STATUS.md)
- [Roadmap](docs/roadmap.md)
- [Multi-backend STT](docs/MULTI_BACKEND_ARCHITECTURE.md)
- [Wake word](docs/WAKE_WORD_ARCHITECTURE.md)
- [Разработка и release](docs/development.md)
- [Ручное тестирование](docs/testing.md)

## Приватность

Аудио и STT остаются на устройстве. Текст отправляется наружу только если
пользователь явно настроил внешний OpenAI-совместимый LLM endpoint; LM Studio
по умолчанию работает локально.
