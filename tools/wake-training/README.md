# Подготовка обучения фиксированной фразы — FONO-144 / W-03

Состояние на 21 октября 2026 года: готова **подготовка локальных данных и
шаблонов конфигурации**. Обучение, ONNX-export и quality qualification не
выполнены. Веса, TTS и датасеты не загружались, Python-пакеты не устанавливались.
WakeWord приложения выключен. Это не законченный end-to-end trainer.

## Что запускается сейчас

Python 3.11+, только стандартная библиотека. Вход —
[корпус W-02](../wake-bench/README.md) `fono-wake-corpus@1`: PCM16 mono 16 kHz,
SHA256, временные аннотации, непрозрачные ID говорящего/сессии/источника,
декларации источника и разрешения. Фактические права проверяет оператор.

```powershell
python tools/wake-training/prepare.py inspect --corpus C:/corpus/fono/corpus.json
python tools/wake-training/prepare.py prepare --corpus C:/corpus/fono/corpus.json `
  --output-dir build/wake-training/round-01
python -m unittest discover -s tools/wake-training -p test_prepare.py -v
```

`inspect` ничего не записывает. Код 0 означает наличие положительных и
речевых отрицательных train/dev примеров обоих языков; код 2 — недостающие
классы; код 1 — ошибка входа. `data_ready` означает только структурную
готовность данных для подготовки, не достаточный объём, разнообразие,
утверждение лицензий или разрешение на обучение.

`prepare` принимает только **новый** каталог. Уже существующий не изменяется.
Положительные окна вырезаются по аннотации, без обрезания фразы до 2 с:
более длинная фраза требует отдельного решения по окну и feature pipeline.
Негативы делятся на последовательные окна по 2 с, последний остаток
сохраняется; padding/alignment относятся к будущей augmentation.
Фоновая тишина имеет отдельный класс. Ничего не определяется через RMS.

У LiveKit каталоги validation называются `*_test`. Здесь в них идут **только
Fono dev**, а Fono test не копируется и не используется для подготовки,
обучения или выбора порога. Общие speaker/session/source_group и одинаковые
исходные WAV между split запрещает общий валидатор W-02. Производные от
одной исходной записи должны сохранять её group во входном manifest.

```text
round-01/
  ru.json                       # шаблон config (JSON — допустимый YAML)
  en.json
  output/fono_ru/
    positive_train/*.wav
    positive_test/*.wav         # только Fono dev
    negative_train/*.wav
    negative_test/*.wav         # только Fono dev
    background_train/*.wav      # если есть idle train
    background_test/*.wav       # если есть idle dev
  output/fono_en/...
  preparation.json             # записывается последним
```

Manifest фиксирует SHA256 корпуса, скриптов, конфигов, выходных WAV,
ID/хэш исходника, исходный split и границы сэмплов. Нет путей входа,
имён людей, текстов и source/license строк. Исходный корпус и доказательства
прав хранятся отдельно локально. WAV проверяется повторно перед извлечением.
Ошибка оставляет незаконченный новый каталог **без** `preparation.json`;
такой каталог не использовать. Скрипт не удаляет его автоматически.

## Конфиги и выбранное направление

[ru.json](ru.json) и [en.json](en.json) — два независимых tiny DNN-кандидата
для «эй фоно» и «hey fono», поверх замороженного speech embedding.
5000 основных шагов — начальный шаблон, не согласованный бюджет и не
результат подбора. Автогенерация и внешние noise/RIR paths выключены,
ACAV100M не включён. Near-miss фразы зафиксированы как будущий список;
скрипт их не синтезирует. Фоновые WAV становятся отдельным классом.
Исходный quality gate W-02 остаётся: quiet recall ≥95%, noisy ≥90%,
≤0,5 FA/h, p95 ≤800 мс, idle CPU <1% одного ядра.

Проверены интерфейсы
[LiveKit WakeWord @95448a7](https://github.com/livekit/livekit-wakeword/tree/95448a7559c453fcd87645bd67b247ffb45f85b0):
[config](https://github.com/livekit/livekit-wakeword/blob/95448a7559c453fcd87645bd67b247ffb45f85b0/src/livekit/wakeword/config.py),
[trainer](https://github.com/livekit/livekit-wakeword/blob/95448a7559c453fcd87645bd67b247ffb45f85b0/src/livekit/wakeword/training/trainer.py),
[augmentation layout](https://github.com/livekit/livekit-wakeword/blob/95448a7559c453fcd87645bd67b247ffb45f85b0/docs/augmentation.md).
Это кандидат upstream, а не подключённая зависимость Fono.

При этом у проверенного commit есть видимое несоответствие API:
[`run_extraction(config, sess_options)`](https://github.com/livekit/livekit-wakeword/blob/95448a7559c453fcd87645bd67b247ffb45f85b0/src/livekit/wakeword/data/features.py)
требует второй аргумент, а
[`cli.augment`/`cli.run`](https://github.com/livekit/livekit-wakeword/blob/95448a7559c453fcd87645bd67b247ffb45f85b0/src/livekit/wakeword/cli.py)
передают один. Это вывод из исходников; upstream pipeline здесь не запускался.
До end-to-end прогона нужен рабочий pin либо локальный исправляющий patch.
Шаблоны не объявляются проверенным запуском этой версии.

## Что нужно перед реальным запуском

1. Локальный разрешённый train/dev корпус с позитивами обоих языков,
   обычной речью, фоном и трудными негативами. Синтетика допустима для train.
   Оценка качества — только на реальных отложенных голосах; нужны корпус
   W-02 и согласованная разметка, включая 100–300 реальных произнесений и
   ≥10 часов реальных негативов. Записи голоса владельца сейчас не требуются.
2. Согласовать точные embedding/TTS/voice/data artifacts и их условия;
   зафиксировать revision, SHA256 каждого файла, полные notices и основания
   использования синтетического голоса/данных. Разбор кандидатов — в
   [THIRD_PARTY_NOTICES](../../THIRD_PARTY_NOTICES.md).
3. Выбрать рабочий upstream pin; получить dependency lock с hashes и версии
   Python/PyTorch/ORT. Зафиксировать random seeds, deterministic settings,
   CPU/GPU, ограничения времени/VRAM/диска и согласованный бюджет. Сейчас
   bit-for-bit воспроизводимость **обучения** не заявляется.
4. Отдельно выполнить augmentation → embedding extraction → train → ONNX
   export. У upstream `setup` есть крупные загрузки, `generate` вызывает TTS,
   а `run` содержит весь pipeline; они не входят в безопасную подготовку выше.
   Отчёт trainer по validation clips не заменяет потоковую оценку W-02.

После появления первого кандидата расширить W-02 для streaming score 0..1,
зафиксировать operating point на dev и один раз оценить неизменённый test.
W-02 сейчас воспроизводит ASR baseline, новый classifier ещё не подключён.
Короткий replay и синтетический train/dev не подтверждают recall, длинные
false activations или CPU/latency конечного каскада.

Трудные негативы брать только из train/dev: сохранять parent run/model hashes,
ID/границы события, source group и новую версию корпуса для каждого раунда.
Ошибки test не возвращать в обучение; после такого использования нужен новый
независимый test. Число итераций и результаты записываются по фактическим
прогонам, сейчас их **0**.

После quality gate подготовить отдельный artifact manifest: ID/версия модели,
HTTPS URL, размер/SHA256 head и embedding/frontend, точные phrases/sample rate/
tensor contract, license/notice, хэши config/corpus/environment и test-отчёта.
Добавлять registry entry по образцу `models.rs` только для существующего
проверенного артефакта. Заглушки SHA и фиктивные download URL не создавать.
Публикация и интеграция FONO-145 — отдельные шаги.
