# Roadmap Fono

## P0 — выпуск переключаемого STT

- [x] Рабочая диктовка, wake word, overlay, VAD и отмена.
- [x] Privacy/injection hardening.
- [x] CUDA worker и реальный CUDA smoke-test.
- [x] Vulkan worker на актуальном whisper.cpp и реальный Vulkan smoke-test.
- [x] UI Auto/CUDA/Vulkan/CPU без молчаливого CPU fallback.
- [x] GUI worker-процессы без всплывающего Terminal.
- [x] Sherpa BPE-карта для `okay fun` и WAV smoke-test.
- [ ] NSIS/MSI acceptance на чистой Windows.
- [ ] AMD/Intel Vulkan acceptance.
- [ ] CI: format, clippy, Rust/TypeScript tests, workers build и installer smoke-test.

## P1 — polish перед широким использованием

- [ ] First-run: микрофон → модель → acceleration → тест → hotkey.
- [ ] Экран readiness/diagnostics: выбранный backend, worker, модель, драйвер
  и понятная причина fallback.
- [ ] Явная UX-подсказка об ограничениях Sherpa для arbitrary wake phrase.
- [ ] Code signing и updater после появления стабильного installer workflow.

## P2 — Command Agent через LM Studio

Цель: распознавать свободную формулировку только после явной команды или
command-hotkey, не предоставляя LLM доступ к shell/PowerShell.

- [ ] `CommandRouter`: сначала локальные правила, затем LLM fallback.
- [ ] Строгая JSON-схема function calls, валидация аргументов и allowlist tools.
- [ ] `set_volume(0..100)`, media controls, `open_app(name)` и
  `focus_window(query)`.
- [ ] Подтверждение overlay для потенциально опасных действий.
- [ ] Понятный результат/ошибка команды в overlay и логах.

## Принцип

Обычная диктовка никогда не интерпретируется как команда. Даже Agent может
только выбрать разрешённый инструмент с валидированными аргументами.
