# Статус Fono

**Версия:** v0.4.0-wakeword
**Дата:** 8 июля 2026
**Статус:** Рабочий MVP — можно пользоваться каждый день

---

## ✅ Что работает прямо сейчас

### 🎙️ Голосовой ввод (полный цикл)
- **Push-to-talk:** зажми `Ctrl+Space` → говори → отпусти → текст вставился в активное окно
- **Wake word:** скажи «Эй, ассистент» → начнётся запись → по паузе текст вставится
- **Текст-инъекция** в любое окно (Notepad, Word, браузер, Telegram, IDE) через Win32 SendInput
- **Русский язык** поддерживается полностью (Unicode)

### 🧠 Транскрипция (whisper.cpp)
- Локальная, 100% офлайн, приватно
- Модели: tiny / base / small / medium / large-v3 (выбор в UI)
- **Оптимизировано под CPU:** AVX2/FMA инструкции включены
  - `base` = ~1 сек на 4 сек аудио
  - `small` = ~3 сек на 4 сек аудио
- VAD обрезает тишину → whisper работает быстрее

### 🛠️ Инструменты в UI
- **Тест микрофона** с индикатором уровня (peak/RMS в dB)
- **Тест транскрипции** с кнопкой записи и показом timing (RTF, устройство)
- **Чекбокс «Вставлять в активное окно»** для теста
- **Менеджер моделей** — скачивание whisper-моделей из UI
- **Проверка соединения с LM Studio**
- **Просмотр логов** прямо в приложении (кнопка «Показать логи»)
- **Логирование в файл** с timestamps (`%APPDATA%\Fono\logs\`)

### ⚙️ Настройки (сохраняются в `%APPDATA%\Fono\settings.json`)
- Выбор микрофона
- Язык распознавания (ru/en/auto)
- Модель whisper
- Горячая клавиша push-to-talk
- Wake word вкл/выкл + фраза
- Режим AI (off/clean/format/command)
- URL LM Studio

---

## ⬜ Что осталось сделать

### Этап 5 — AI-постобработка (СЛЕДУЮЩИЙ)
LM Studio клиент уже написан, нужно подключить к pipeline.
- Чистка «ээ/мм», пунктуация, форматирование
- Команды («преврати в email», «переведи»)
- Стриминг ответа

### Этап 6 — UX polish
- Onboarding wizard
- Overlay с анимацией
- Автозапуск с Windows
- Дебаунс wake word

### Этап 7 — Упаковка
- MSI/NSIS инсталлятор
- Подпись кода
- Автообновление

### Будущее (опционально)
- **GPU-ускорение** (CUDA/Vulkan) — features уже в Cargo.toml, нужно починить сборку с MSVC
- **Streaming** — текст по мере речи
- **macOS/Linux** порты

---

## 📊 Производительность

### Транскрипция (i5-12600KF, CPU, AVX2)

| Модель | Размер | Время на 4 сек аудио | RTF | Качество RU |
|---|---|---|---|---|
| tiny | 75 МБ | ~0.3 сек | 0.08x | Слабое |
| base | 147 МБ | ~1 сек | 0.25x | Приемлемое |
| small | 460 МБ | ~3 сек | 0.75x | Хорошее |
| medium | 1.5 ГБ | ~8-10 сек | 2.5x | Отличное |
| large-v3 | 3 ГБ | ~60 сек | 15x | SOTA |

**Рекомендуемая:** `small` для повседневной работы (баланс скорости/качества).

### Wake word
- CPU в idle: ~5-10%
- Задержка обнаружения: ~2 сек (1.5 сек чанк + 0.3 сек транскрипция)

---

## 🏗️ Архитектура (краткая)

```
fono/
├── src-tauri/src/
│   ├── audio/         # cpal (WASAPI), универсальный конвертер форматов
│   ├── stt/           # whisper.cpp (whisper-rs 0.16), AVX2/FMA оптимизация
│   ├── vad/           # energy-based VAD, trim_silence
│   ├── wakeword/      # WakeWordDetector, whisper continuous
│   ├── injection/     # Win32 SendInput (KEYEVENTF_UNICODE)
│   ├── llm/           # HTTP-клиент к LM Studio (написан, ждёт интеграции)
│   ├── pipeline/      # FSM: Idle→Listening→Transcribing→Processing→Injecting
│   ├── commands.rs    # Tauri IPC команды
│   └── state.rs       # AppState, settings.json
├── src/
│   ├── views/         # Settings, Onboarding, Overlay
│   ├── components/    # MicTest, ModelManager, StatusBadge
│   └── lib/           # ipc.ts, types.ts
└── docs/              # architecture, development, roadmap, STATUS
```

---

## 🔧 Сборка и запуск

```bash
# ВСЕГДА через tauri build (не cargo build!), иначе фронтенд не встроится
export PATH="/c/Users/0whit/.cargo/bin:/c/Program Files/CMake/bin:$PATH"
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
npm run tauri build -- --no-bundle
./src-tauri/target/release/fono.exe
```

Dev-режим (с горячей перезагрузкой UI):
```bash
npm run tauri dev
```

---

## 🐛 Известные проблемы

1. **GPU не работает** — сборка whisper.cpp с Vulkan/CUDA падает на MSVC v18.
   Features добавлены, ждут отладки. CPU сейчас достаточно быстр.

2. **VAD в wake dictation упрощён** — после wake word запись идёт фиксированное
   время (пока заглушка), а не по реальному VAD-мониторингу. Нужно допилить.

3. **Wake word + push-to-talk конфликт** — если оба активны, могут мешать друг
   другу. Нужно добавить взаимную блокировку.

4. **LM Studio не интегрирован в pipeline** — клиент написан, но не вызывается
   автоматически. Сейчас только ручной тест соединения.

---

## 📝 Git

```
2918248 (HEAD -> main, tag: v0.4.0-wakeword) feat: Wake word
...       feat: REAL AVX2/FMA для whisper.cpp
...       feat: Push-to-talk + VAD
...       feat: Текст-инъекция
...       feat: Транскрипция MVP
...       feat: Скаффолд
```

Чекпойнты:
- `v0.4.0-wakeword` — актуальный (wake word + push-to-talk + оптимизации)
- `stable_mvp` — ветка с рабочей транскрипцией (без wake word)
