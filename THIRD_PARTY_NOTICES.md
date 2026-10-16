# Сторонние компоненты Fono

Корневой [MIT](LICENSE) распространяется на собственный код и документацию Fono.
Сторонние библиотеки, модели, runtime и исходники сохраняют свои лицензии и
copyright notices. Наличие MIT у Fono не означает, что весь установщик имеет одну
лицензию. Название и графика описаны в [отдельных условиях](docs/brand-assets.md).

Это исходный реестр и результаты аудита от 17 октября 2026 года. Он отделяет
кодовые зависимости из lock-файлов от реально подготовленных native-файлов и
моделей, которые загружаются отдельно. Закрытие перечисленных ниже вопросов и
проверка включения notices в конкретный установщик ещё требуются.

## Полные тексты и воспроизводимый реестр

- [Инвентарь npm и Cargo](licenses/dependency-inventory.json) содержит точные
  версии из `package-lock.json`, `src-tauri/Cargo.lock` и
  `tools/fono-setup/Cargo.lock`, заявленные SPDX-выражения, пути notices внутри
  пакетов и SHA-256 исходных текстов. Выражения `OR` и `AND` сохранены как у автора.
- [Собранные полные notices](licenses/dependency-notices.txt) содержат тексты из
  доступных локальных пакетов и vendored whisper.cpp. По SHA-256 из JSON можно
  найти нужный текст. Сохранены в том числе вложенные лицензии Opus и других
  native-исходников; это не только список лицензий верхнего уровня.
- [Источники дополнительных текстов](licenses/upstream-sources.json) связывают
  файлы `licenses/upstream/` с upstream URL, версиями и хэшами снимков.
- [Native-файлы и модели](licenses/native-inventory.json) содержат отдельные
  хэши и состояние проверки. Снимок локальных DLL не является подтверждением
  состава ранее опубликованного установщика.

Полный lock-инвентарь включает build/dev-зависимости, optional npm-пакеты и
другие ОС/архитектуры. Они не объявляются поставляемыми Windows-компонентами.
`dev`, `optional`, `os`, `cpu` и `sourceStatus` помогают отделить npm-пакеты;
Cargo lock сам по себе не определяет использованные features и linked objects.
Отсутствующий локальный notice отмечен в `issues` как пробел доказательств,
а не автоматически как нарушение в распространяемом бинарнике.

Обновление без сборки и загрузки пакетов, Python 3.11+:

```powershell
python scripts/license-inventory.py --write
python scripts/license-inventory.py --check
python scripts/license-inventory.test.py
```

`--check` проверяет соответствие всех трёх lock-файлов сохранённому снимку.
Успешная проверка не закрывает записанные в JSON пробелы. После изменения lock
нужно повторить сбор текстов из установленных пакетов и Cargo cache.

## Основные библиотеки приложения

| Компонент                         | Зафиксированная версия  | Лицензия и notices                                                                                                                                                                                                                                                    |
| --------------------------------- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Vue / Vue Router                  | 3.5.13 / 4.6.3          | MIT; полные notices включены в собранные тексты                                                                                                                                                                                                                       |
| Tauri API                         | 2.11.1                  | Apache-2.0 OR MIT; полные notices включены                                                                                                                                                                                                                            |
| WhiteUI `@whitelife-core/ui-kit`  | 0.6.0                   | Установленный `package.json` и lock заявляют MIT; полного LICENSE/copyright notice в пакете и проверенном локальном upstream нет. Требуется notice владельца WhiteUI                                                                                                  |
| Tauri / global-shortcut / updater | 2.11.5 / 2.3.2 / 2.12.0 | Apache-2.0 OR MIT; точные тексты и остальные crates перечислены в Cargo-инвентаре                                                                                                                                                                                     |
| whisper-rs / whisper-rs-sys       | 0.16.0 / 0.15.0         | Rust bindings: Unlicense. Включённый whisper.cpp имеет отдельный MIT ggml authors; сохранён из содержимого crate                                                                                                                                                      |
| Symphonia и включённые codecs     | 0.5.5                   | MPL-2.0; [полный текст](licenses/upstream/symphonia-0.5.5-MPL-2.0.txt), [исходники v0.5.5](https://github.com/pdeljanov/Symphonia/tree/v0.5.5). Изменения этих файлов сохраняют MPL; исходный код компонента должен оставаться доступным получателю согласно лицензии |
| audiopus / audiopus_sys           | 0.2.0 / 0.1.8           | ISC для bindings; bundled Opus сохраняет собственный BSD-style `COPYING`, включённый в собранные notices                                                                                                                                                              |
| WebView2 Rust bindings            | webview2-com 0.38.2     | MIT; [notice точного исходного commit crate](licenses/upstream/webview2-rs-0.38.2-MIT.txt). Это не лицензия Microsoft WebView2 Runtime                                                                                                                                |

WhiteUI остаётся приватным GitHub Packages-пакетом. Этот аудит не публикует его
исходники, не подтверждает авторские права на него и не делает `npm ci` доступным
без токена. Открытая сборка относится к OSS-02.

## Native-библиотеки и GPU

Vulkan worker использует отдельный submodule
`ggml-org/whisper.cpp@7695a5331230c585f5ce92291c4256973985ae5a`, а Rust bindings
содержат свою копию whisper.cpp. Обе копии имеют MIT, но notices и годы copyright
у них различаются и сохранены отдельно. В ggml также есть MIT-код Mozilla
Foundation (llamafile) и алгоритм YaRN Jeffrey Quesnelle/Bowen Peng; дополнительные
уведомления сохранены в `licenses/upstream/` и не заменены MIT Fono.

Sherpa wrappers `sherpa-onnx`/`sherpa-onnx-sys` 1.13.4 и основной upstream имеют
[Apache-2.0](licenses/upstream/sherpa-onnx-1.13.4-APACHE-2.0.txt). Это не исчерпывает
состав native DLL. Зафиксированный архив в `scripts/ci-native-dependencies.json`
и release-подготовка поставляют `sherpa-onnx-c-api.dll`,
`sherpa-onnx-cxx-api.dll`, `onnxruntime.dll`, `onnxruntime_providers_shared.dll`.
Проверенные локальные ORT DLL сообщают версию **1.27.0**; сохранены её
[MIT](licenses/upstream/onnxruntime-1.27.0-MIT.txt) и полный
[ThirdPartyNotices](licenses/upstream/onnxruntime-1.27.0-ThirdPartyNotices.txt).
Перечень ORT notices общий для upstream; он не доказывает включение каждого
перечисленного provider в эти DLL.

Важный остаток по Sherpa: [CMake v1.13.4](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/CMakeLists.txt)
включает TTS по умолчанию. [Рецепт espeak](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/cmake/espeak-ng-for-piper.cmake)
фиксирует `ed530aa113046142eb5115cf2fc9157854d0ffe1` и статически собирает его для
shared Sherpa; [piper-phonemize](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/cmake/piper-phonemize.cmake)
имеет MIT. В подготовленном `sherpa-onnx-c-api.dll` обнаружены строки eSpeak NG и
phonemization. Сохранены [GPL v3 espeak](licenses/upstream/espeak-ng-ed530aa-GPL-3.0.txt)
и [MIT piper](licenses/upstream/piper-phonemize-f3ff95a-MIT.txt). Нужна проверка
точного build provenance архива, соответствующего исходного кода и условий
распространения составной DLL. Отключённый WakeWord или неиспользуемый TTS сами по
себе не удаляют код из бинарника. Этот вопрос нельзя закрыть добавлением MIT Fono.

CUDA worker копирует `cublas64_13.dll`, `cublasLt64_13.dll`, `cudart64_13.dll` из
CUDA Toolkit. Локальный toolkit и CI pin — **13.3.1**; у отдельных библиотек свои
номера версий, записанные в native-инвентаре. Они остаются proprietary NVIDIA
redistributables, а не MIT. Сохранены точные установленные
[EULA](licenses/upstream/NVIDIA-CUDA-13.3.1-EULA.txt) и
[LICENSE/third-party terms](licenses/upstream/NVIDIA-CUDA-13.3.1-third-party.txt).
[NVIDIA CUDA EULA](https://docs.nvidia.com/cuda/eula/index.html) включает CUDA
Runtime и BLAS в Attachment A, включая варианты имён с номером версии, и задаёт
условия распространения. Перед выпуском нужны применимые условия для получателя
и проверка конкретного пакета; согласие владельца Fono на MIT не заменяет их.

Vulkan SDK 1.4.350.0 используется для сборки. Worker разрешает Vulkan через
установленный графический драйвер; текущий staging не копирует Vulkan SDK или
драйвер целиком. При добавлении loader/runtime DLL понадобятся их собственные
notices; лицензия whisper.cpp их не покрывает.

## Модели и данные

| Артефакт                                                           | Поставка                                                                                      | Отдельная лицензия/основание                                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whisper tiny, base, small, medium, large-v3, large-v3-turbo в GGML | Загружаются отдельно с `ggerganov/whisper.cpp`, в Git отсутствуют                             | OpenAI [явно лицензирует код и model weights по MIT](https://github.com/openai/whisper#license); [полный notice](licenses/upstream/whisper-openai-MIT.txt)                                                                                                                   |
| Silero VAD `silero_vad.onnx`                                       | Включён в ресурсы, SHA-256 `9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6` | [Silero VAD MIT, Silero Team](licenses/upstream/silero-vad-MIT.txt); это репозиторий `silero-vad`, не другие Silero STT/TTS модели                                                                                                                                           |
| T-one RU `sherpa-onnx-streaming-t-one-russian-2025-09-08`          | Отдельно загружаемый pinned wake/streaming archive; WakeWord выключен                         | Apache-2.0, Copyright 2025 T-Software DC; [notice](licenses/upstream/t-one-APACHE-2.0-notice.txt), [полный Apache-2.0](licenses/upstream/icefall-APACHE-2.0.txt), [model card явно включает pretrained models](https://huggingface.co/t-tech/T-one)                          |
| EN `sherpa-onnx-streaming-zipformer-en-2023-06-26`                 | Отдельно загружаемый pinned archive                                                           | Apache-2.0 в [model card издателя](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26); сохранена также декларация из README локального model pack                                                                                              |
| KWS `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`         | Исторический архив; backend и downloader удалены в W-01                                       | README локального model pack заявляет Apache License 2.0 и происхождение icefall/GigaSpeech; [модель издателя](https://www.modelscope.cn/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01), [полный Apache-2.0](licenses/upstream/icefall-APACHE-2.0.txt) |

Archive URL и хэши двух исследовательских streaming packs сохраняются в
`src-tauri/crates/fono-wake/src/models.rs`. GigaSpeech KWS удалён в W-01; его
историческая декларация доступна в [исходниках v0.7.0](https://github.com/whitewolf06/fono/blob/e91f201eb96fab40151a9d228fecab5c2b18f6a9/src-tauri/crates/fono-wake/src/models.rs). Лицензия icefall на тренировочный код
сама по себе не лицензирует любые обученные веса или исходные датасеты. Здесь
модельные декларации проверены отдельно; тренировочные датасеты и sample WAV не
объявляются MIT Fono. Для перераспространения тестовых WAV/нового корпуса нужна
проверка их собственных условий. RU archive содержит LICENSE; EN pack в
исследовательском downloader не требует отдельного LICENSE-файла — notices нужно сохранять
при подготовке распространяемого набора моделей.

## Microsoft и установщики

MSVC runtime/Windows SDK и Microsoft WebView2 Runtime имеют собственные условия.
Fono настраивает Tauri `downloadBootstrapper`; наличие MIT у Rust WebView2
bindings не переименовывает Microsoft runtime в open source. Основания:
[Visual C++ redistribution](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files)
и [WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).
Онлайн-загрузчик использует static CRT. Разрешение конкретного developer toolchain
на распространение CRT и состав фактического NSIS/MSI нужно проверить перед
выпуском. В этом изменении установщик не пересобран и не объявлен проверенным.

## Что осталось до полной приёмки OSS-01

1. Получить от владельца WhiteUI полный MIT copyright/permission notice для 0.6.0.
2. Закрыть состав и redistribution/source obligations Sherpa native DLL, включая
   eSpeak, и подтвердить provenance именно используемого архива.
3. Закрыть относящиеся к Windows runtime пробелы `issues`, проверить source access
   для MPL-компонентов и точный набор native notices. Отдельно оставить
   build/dev/другие платформы, которые не попадают в этот выпуск.
4. Включить `LICENSE`, этот файл и применимые полные notices в каждый
   распространяемый EXE/installer/package; проверить состав готового пакета.
   При необходимости отдельно оформить Microsoft/NVIDIA условия для получателя.

Этот реестр не меняет лицензии сторонних авторов и не подменяет эту приёмку.
