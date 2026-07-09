# Архитектура wake word

Этот документ описывает предлагаемую переработку wake word для Fono.

Текущую реализацию wake word нужно считать экспериментальной. Она основана на коротких аудио-фрагментах, распознавании через Whisper и поиске wake-фразы в полученном тексте. Такой подход может работать как временный fallback, но он слишком тяжелый для отзывчивой функции, которая должна постоянно слушать микрофон и быстро реагировать на ключевую фразу.

Предлагаемое направление: вынести wake word в отдельный Rust-модуль/crate и использовать легкий keyword spotting backend на базе `sherpa-onnx`.

## Короткое резюме

Целевая схема:

```text
микрофон
-> легкий wake word detector
-> событие "фраза услышана"
-> обычная диктовка через Whisper
-> optional LLM cleanup
-> вставка текста или выполнение команды
```

Whisper остается основным движком для качественной диктовки. Wake word должен быть отдельным быстрым слоем, а не постоянным Whisper-распознаванием коротких чанков.

Рекомендуемая стратегия:

```text
Hotkey диктовка = стабильное ядро MVP
Текущий Whisper wake word = experimental fallback
Новый sherpa-onnx wake word = отдельный planned backend
```

## Цели

- Сохранить основной сценарий диктовки быстрым и стабильным.
- Не запускать Whisper постоянно только ради поиска wake-фразы.
- Изолировать wake word от остального приложения.
- Сделать wake word необязательной функцией, которая не ломает hotkey-диктовку.
- Сохранить local-first и open-source friendly подход.
- Подготовить код к будущему smart speaker / assistant mode.
- Разрешить несколько wake word backend-ов без переписывания desktop pipeline.
- Сделать архитектуру понятной для open-source и портфолио.

## Не цели

- Не писать собственную ML-модель keyword spotting с нуля.
- Не собирать и не обучать датасет wake word внутри этого проекта.
- Не делать wake word обязательной зависимостью для MVP.
- Не блокировать полировку текущего приложения на качестве wake word.
- Не заменять Whisper для обычной диктовки.
- Не превращать приложение в отдельный transcription suite или ассистента на этом этапе.

## Текущая проблема

Сейчас wake word концептуально работает примерно так:

```text
аудио с микрофона
-> накопить короткий фрагмент
-> прогнать через Whisper
-> сравнить распознанный текст с wake-фразой
-> если похоже, отправить wake event
```

Практические проблемы такого подхода:

- Whisper предназначен для speech-to-text, а не для низколатентного keyword spotting.
- Системе нужно дождаться накопления достаточного аудио-фрагмента.
- Короткие wake-фразы легко пропускаются или распознаются неточно.
- Первый inference после простоя может быть медленным.
- Постоянный Whisper может конкурировать с основным STT pipeline за CPU/GPU.
- Задержка в 3-5 секунд для wake word воспринимается как поломка.
- Короткая фраза может быть частично обрезана VAD-ом.
- Fuzzy matching по тексту не дает надежной вероятностной оценки.

Для wake word целевая задержка должна быть существенно ниже:

```text
хорошая цель: 100-700 мс после произнесения фразы
приемлемая MVP-цель: обычно меньше 1 секунды
```

## Предлагаемая архитектура

Wake word должен быть отдельной подсистемой с небольшим Rust API.

```text
microphone stream
-> fono-wake
+  -> выбранный wake backend
+  -> wake event
-> desktop pipeline
+  -> pause wake listener
+  -> start dictation
+  -> Whisper STT
+  -> optional LLM cleanup
+  -> insert text или execute command
+  -> resume wake listener
```

Основное приложение не должно знать детали `sherpa-onnx`, ONNX-сессий, файлов модели или threshold-логики. Оно должно получать только высокоуровневые события: `Detected`, `Listening`, `Paused`, `Error`.

## Рекомендуемая структура модулей

Предпочтительный вариант: отдельный Rust crate внутри Tauri workspace.

```text
src-tauri/
  crates/
    fono-wake/
      Cargo.toml
      src/
        lib.rs
        config.rs
        engine.rs
        event.rs
        error.rs
        audio_buffer.rs
        backend/
          mod.rs
          disabled.rs
          whisper_experimental.rs
          sherpa_onnx.rs
          mock.rs
```

Основное Tauri-приложение подключает этот crate:

```toml
[dependencies]
fono-wake = { path = "crates/fono-wake" }
```

## Публичный API

Главное приложение должно зависеть от стабильного интерфейса, а не от деталей `sherpa-onnx`.

```rust
pub trait WakeWordEngine {
    fn start(&mut self) -> Result<(), WakeWordError>;
    fn stop(&mut self) -> Result<(), WakeWordError>;
    fn pause(&mut self) -> Result<(), WakeWordError>;
    fn resume(&mut self) -> Result<(), WakeWordError>;
    fn status(&self) -> WakeWordStatus;
}
```

События:

```rust
pub enum WakeWordEvent {
    Listening,
    Paused,
    Detected {
        phrase: String,
        confidence: f32,
    },
    Error {
        message: String,
    },
}
```

Конфигурация:

```rust
pub struct WakeWordConfig {
    pub enabled: bool,
    pub backend: WakeWordBackend,
    pub phrase: String,
    pub model_dir: PathBuf,
    pub threshold: f32,
    pub sensitivity: f32,
    pub sample_rate: u32,
    pub cooldown_ms: u64,
}
```

Backend-ы:

```rust
pub enum WakeWordBackend {
    Disabled,
    WhisperExperimental,
    SherpaOnnx,
}
```

## Стратегия backend-ов

### Disabled

Стабильный вариант по умолчанию для MVP.

Используется, когда wake word выключен. Основная hotkey-диктовка не должна зависеть от wake word.

### WhisperExperimental

Текущую реализацию можно сохранить как fallback и как промежуточный backend.

Ее нужно явно пометить как experimental, потому что она ожидаемо медленнее и менее надежна, чем настоящий keyword spotting engine.

Роль:

- временная совместимость;
- fallback для разработки;
- режим, если sherpa-onnx assets не установлены;
- не recommended default.

### SherpaOnnx

Предлагаемый правильный wake word backend.

Роль:

- основной будущий wake word engine;
- низколатентный keyword spotting;
- local-first и open-source friendly подход;
- подходящая база для будущего desktop assistant и smart speaker mode.

## Почему выбран sherpa-onnx

`sherpa-onnx` подходит, потому что поддерживает локальные ONNX-based speech models, включая keyword spotting. Важно: он не должен заменять Whisper. Он должен решать узкую задачу: быстро понять, была ли сказана wake-фраза.

Причины выбора:

- работает локально, без облачного распознавания;
- лучше подходит для open-source, чем vendor SDK;
- можно спрятать за аккуратной Rust-обвязкой;
- архитектурно лучше, чем постоянный Whisper;
- потенциально лучше готовит проект к Windows/macOS/Linux, чем Windows-only хаки;
- подходит для будущей умной колонки;
- не требует писать ML-инференс с нуля;
- можно оставить опциональным backend-ом через feature flags.

## Почему не Porcupine как основной путь

Porcupine технически привлекателен: он быстрый, зрелый и хорошо подходит для wake word. Но для этого проекта он хуже как дефолтный open-source путь.

Проблемы:

- vendor dependency;
- лицензионные ограничения;
- хуже выглядит для публичного open-source портфолио;
- менее подходит для будущего fully-local smart speaker эксперимента.

Porcupine можно оставить как возможный optional backend позже, но не как основную архитектуру.

## Почему не openWakeWord первым

openWakeWord концептуально хорошо совпадает с задачей, но может усложнить упаковку Rust/Tauri-приложения, особенно на Windows.

Риски:

- возможная Python-инфраструктура;
- сложнее packaging;
- непонятный размер дистрибутива;
- больше friction при интеграции в Rust desktop app.

Это хороший второй кандидат, если `sherpa-onnx` не подойдет по качеству или интеграции.

## Почему не собственная ML-реализация с нуля

Полностью свой wake word engine потребовал бы:

- записи wake word;
- negative samples;
- датасета;
- обучения модели;
- inference runtime;
- feature extraction;
- тестирования false positives / false negatives;
- тюнинга threshold;
- packaging model artifacts.

Это отдельный ML-исследовательский проект. Для текущей цели, где нужно дополировать полезное Windows-приложение, это слишком дорогой путь.

## Зависимости

Финальный список зависит от выбранного способа интеграции, но ожидаемые группы такие.

### Internal Rust crate

```text
fono-wake
```

Ответственность:

- публичный wake word API;
- выбор backend-а;
- mapping app settings -> wake config;
- event emission;
- lifecycle pause/resume/stop;
- error handling;
- mock/test backend.

### Sherpa / ONNX runtime layer

Ответственность:

- загрузка KWS model files;
- streaming inference;
- обнаружение keyword phrase;
- confidence/score, если доступно;
- ошибки инициализации модели.

Возможные пути интеграции:

```text
Rust wrapper -> sherpa-onnx C API
Rust wrapper -> bindgen FFI
Rust wrapper -> существующие Rust bindings, если они достаточно зрелые
```

Предпочтительный путь нужно выбрать после небольшого spike/prototype, а не до него.

### Model assets

Для KWS backend понадобятся model assets.

Возможные файлы:

- ONNX model files;
- tokens file;
- keywords config;
- backend metadata;
- возможно encoder/decoder/joiner, зависит от конкретной модели.

Большие model assets не должны попадать в git. Их нужно скачивать или выбирать так же, как Whisper models.

### Audio input

В приложении уже есть microphone capture через CPAL. Wake word желательно встроить так, чтобы не плодить несовместимый второй аудио-стек.

Требования:

- mono audio;
- ожидаемый sample rate, часто 16 kHz;
- стабильный frame size;
- работа вне UI thread;
- предсказуемый pause/resume;
- отсутствие конфликта с основной записью диктовки.

## Поток данных

Обычное idle-состояние с включенным wake word:

```text
App starts
-> settings loaded
-> wake word enabled?
-> fono-wake starts selected backend
-> backend listens to microphone frames
-> overlay/tray shows wake status
```

Wake phrase detected:

```text
KWS backend detects phrase
-> fono-wake emits WakeWordEvent::Detected
-> app pauses wake listener
-> app shows overlay
-> app starts normal dictation capture
-> user speaks text/command
-> app runs Whisper STT
-> app optionally runs LLM cleanup/command parsing
-> app injects text or executes command
-> app returns to idle
-> app resumes wake listener
```

Ошибка backend-а:

```text
backend init fails
-> app emits wake-word-status = error
-> UI shows clear message
-> hotkey dictation remains available
```

## Модель настроек

Нужно отделить общие wake word настройки от backend-specific настроек.

Предлагаемая структура:

```rust
pub struct WakeWordSettings {
    pub enabled: bool,
    pub backend: WakeWordBackend,
    pub phrase: String,
    pub sensitivity: f32,
    pub threshold: f32,
    pub cooldown_ms: u64,
    pub model_dir: Option<PathBuf>,
}
```

Рекомендуемые defaults:

```text
enabled: false
backend: Disabled или WhisperExperimental, пока SherpaOnnx не готов
phrase: "hey fono"
sensitivity: conservative
cooldown_ms: 1500-3000
```

Wake word должен оставаться выключенным по умолчанию, пока `SherpaOnnx` backend не станет стабильным.

## UI requirements

Wake word нельзя показывать как полностью стабильную функцию, пока новый backend не внедрен и не протестирован.

Рекомендуемые названия в UI:

```text
Wake word: Off
Wake word: Experimental Whisper
Wake word: Sherpa ONNX
```

Обязательные состояния:

- off;
- listening;
- paused;
- detected;
- error;
- missing model;
- model loading.

Рекомендуемые controls:

- enable/disable wake word;
- выбрать backend;
- выбрать wake phrase;
- sensitivity slider;
- test wake word button;
- показать текущий backend;
- показать model status.

## Error handling

Ошибки wake word не должны ломать основное приложение.

Если wake word упал:

- отключить wake word на текущую сессию;
- показать понятную ошибку;
- оставить hotkey-диктовку доступной;
- записать подробные логи;
- разрешить retry после изменения настроек.

Примеры сообщений:

```text
Wake word model is missing. Download or select a model.
Wake word backend failed to initialize. Hotkey dictation is still available.
Wake word microphone stream failed. Check the selected input device.
```

## Feature flags

Интеграцию нужно сделать так, чтобы core app мог собираться без `sherpa` backend-а.

Пример:

```toml
[features]
default = ["whisper-wake"]
whisper-wake = []
sherpa-wake = []
```

Это сохранит MVP-сборку проще и позволит дорабатывать `sherpa` backend отдельно.

## Критерии качества

Для `SherpaOnnx` backend нужно считать интеграцию успешной только если выполнены базовые критерии.

Минимальные критерии:

- wake phrase уверенно срабатывает в тихой комнате;
- обычная задержка меньше 1 секунды;
- нет заметных подвисаний UI;
- hotkey-диктовка работает даже при ошибке wake word;
- wake listener корректно ставится на паузу во время основной диктовки;
- после завершения диктовки wake listener корректно возобновляется;
- false positives не происходят постоянно на обычную речь;
- модель и runtime понятно диагностируются в логах.

Желательные критерии:

- стабильная работа после долгого idle;
- повторное срабатывание без перезапуска приложения;
- понятный test mode в UI;
- возможность быстро отключить wake word из overlay/tray;
- корректное поведение при смене микрофона.

## Риски

### Packaging

`sherpa-onnx` может потребовать дополнительные native binaries или model assets. Это может усложнить Windows installer.

Митигировать:

- не коммитить большие модели;
- добавить model manager или manual path;
- держать `sherpa-wake` optional;
- сначала сделать standalone prototype.

### Качество модели

Не каждая KWS-модель одинаково хорошо подойдет для пользовательской wake-фразы.

Митигировать:

- сначала тестировать фиксированную фразу;
- не обещать arbitrary custom wake word в MVP;
- добавить sensitivity/threshold;
- собрать локальный тестовый набор WAV-файлов.

### Конфликт аудиопотоков

Wake listener и основная диктовка могут конфликтовать за microphone device.

Митигировать:

- один владелец audio stream, если возможно;
- строгий lifecycle: pause wake -> dictation -> resume wake;
- понятные ошибки при device busy.

### Размер приложения

Модели и runtime могут увеличить размер дистрибутива.

Митигировать:

- не бандлить все модели по умолчанию;
- сделать download/select model;
- документировать минимальную модель.

## План разработки

### Фаза 1. Стабилизировать текущий MVP

- Оставить wake word выключенным по умолчанию.
- Пометить текущий wake word как experimental.
- Убедиться, что hotkey-диктовка не зависит от wake word.
- Улучшить overlay и diagnostics.

### Фаза 2. Вынести wake word interface

- Создать `fono-wake` crate.
- Вынести generic wake word types.
- Добавить `Disabled` backend.
- Добавить `Mock` backend для тестов/manual simulation.
- Обернуть текущую Whisper-реализацию как `WhisperExperimental`.

### Фаза 3. Sherpa-onnx spike

- Создать минимальный standalone prototype.
- Загрузить KWS model.
- Сначала подать WAV file frames.
- Потом проверить microphone streaming.
- Измерить latency.
- Проверить false positives и missed detections.

Success criteria:

```text
wake phrase reliably detected in quiet room
latency usually below 1 second
UI is not blocked
normal dictation is not affected
hotkey mode still works if backend fails
```

### Фаза 4. Интеграция в приложение

- Добавить `SherpaOnnx` backend в `fono-wake`.
- Добавить выбор backend-а в настройки.
- Добавить model status в UI.
- Добавить wake word status в overlay/tray.
- Добавить понятные ошибки и логи.

### Фаза 5. Packaging

- Решить, как пользователь получает KWS model files.
- Не коммитить большие model files.
- Добавить download или manual model path.
- Описать setup в README.

## Open questions

- Какая конкретная `sherpa-onnx` KWS model даст лучший latency/quality баланс?
- Должна ли первая стабильная wake-фраза быть фиксированной?
- Делать wake phrase английской, русской или пользовательской?
- Использовать ли тот же микрофон, что для диктовки, или отдельную настройку?
- Должны ли wake word и dictation делить один audio stream?
- Насколько велики model assets?
- Можно ли чисто упаковать backend в Windows installer?
- Нужно ли в MVP показывать confidence score пользователю или только использовать его для логов?

## Рекомендация

На текущей стадии wake word должен быть вторичной функцией.

Рекомендуемое ближайшее состояние:

```text
Hotkey dictation = stable core
Current Whisper wake word = experimental fallback
New sherpa-onnx wake word = planned isolated backend
```

Правильный путь не в том, чтобы переписать приложение вокруг wake word. Правильный путь: изолировать wake word за `fono-wake`, сохранить основной dictation pipeline стабильным и добавить `sherpa-onnx` как optional backend после того, как MVP polish будет под контролем.
