# Стенд WakeWord — W-02 / FONO-143

Это отдельный исследовательский стенд. Текущий пользовательский режим описан
в [руководстве Лиры beta](../../docs/wake-lira.md). Версии и метрики ниже относятся
к этапам подготовки и не являются общей оценкой качества выпущенной функции.

v0.7.68: для новой русской фразы **«Привет, Лира»** подготовлен отдельный
аудит локальных записей `recordings.py --root КАТАЛОГ --phrase-id privet_lira`.
Он отделяет новую metadata `@2` от прежней `@1` и не измеряет качество модели.
[Инструкция записи](../../docs/archive/improvement-plan/improvement-wake-lira.md). Описанный ниже ASR
benchmark сохраняет исторический контракт «Эй, Фоно» / «Hey Fono» и не
переименовывает его результаты в измерения новой фразы.

Отдельный CLI воспроизводит WAV через текущие `SherpaDetector` и
`DecoderSession`. Он не записывает микрофон, не открывает AudioHub,
не использует Tauri, не скачивает модели и не меняет настройки Fono.
WakeWord приложения остаётся замороженным.

Нужны Python 3.10+ без сторонних пакетов и отдельный Rust example
с feature `sherpa-wake`. По умолчанию example не собирается.

## Сборка на Windows

Из корня репозитория с установленными native build-зависимостями:

```powershell
./scripts/bootstrap-ci-resources.ps1
$env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'src-tauri/target-codex-review'
cargo build --locked --manifest-path src-tauri/Cargo.toml -p fono-wake `
  --no-default-features --features sherpa-wake --example wake_bench `
  --release --message-format=json > build/wake-bench-artifacts.jsonl
node scripts/native-test-runtime.mjs build/wake-bench-artifacts.jsonl `
  $env:SHERPA_ONNX_LIB_DIR $env:CARGO_TARGET_DIR
npm run cache:maintain
```

Проверяйте exit code каждой команды. Для первого запуска каталог `build/`
должен существовать. Runtime helper размещает закреплённые DLL рядом
с executable по путям Cargo. Используйте отдельный процесс стенда;
не запускайте пользовательское Fono ради проверки.

Контрактные тесты не требуют моделей:

```powershell
python -m unittest discover -s tools/wake-bench -p test_bench.py -v
```

В CI выполняется та же команда через `python3`. Метрики CPU на Linux
пока остаются неизвестными; Windows-сборка измеряет время своего процесса.

## Корпус

JSON имеет schema `fono-wake-corpus@1`, `id`, точные `phrases`
`{"ru":"эй фоно","en":"hey fono"}` и массив `entries`. Пример записи:

```json
{
  "id": "ru-quiet-001",
  "wav": "audio/ru-quiet-001.wav",
  "sha256": "SHA256_ФАЙЛА",
  "language": "ru",
  "split": "dev",
  "origin": "real",
  "condition": "quiet",
  "activity": "speech",
  "speaker": "speaker-001",
  "session": "session-001",
  "source_group": "recording-001",
  "permission": "consented",
  "license": "Условия согласия на исследовательское использование",
  "source": "Идентификатор подтверждённого разрешения",
  "phrases": [{ "start_ms": 500, "end_ms": 1200 }]
}
```

Это шаблон, разрешения в нём не предоставлены. Оператор проверяет лицензию
или согласие на каждую запись; программа проверяет наличие декларации,
формат и целостность. У публичного набора `permission=public-license`,
а `source` ссылается на источник и его условия. Используйте обезличенные
ID говорящих/сессий, без имён людей. Исходные WAV и документы согласия
не нужно добавлять в Git.

- Формат — непустой PCM16 mono 16 kHz WAV, не более 64 МиБ на файл.
  Длинную запись разделяйте с сохранением общего `source_group`, чтобы
  её части не попали в разные выборки. Аудио не обрезается молча.
- Негативы имеют `phrases: []`, положительные произнесения размечаются
  по реальным началу/концу фразы. В одном файле может быть несколько
  неперекрывающихся произнесений.
- `origin` — `real` или `synthetic`; `condition` — `quiet` или `noisy`;
  `activity` — `idle` или `speech`. Тишина для проверки CPU — отдельный
  idle-вход. Синтетические позитивы не входят в оценку recall.
- `train`/`dev`/`test` назначаются до прогона. Совпавший SHA256 аудио,
  говорящий, сессия или исходная запись между выборками отвергаются.
  Хэш проверяется до и после replay; модели/runtime/runner также
  фиксируются и повторно проверяются перед сохранением отчёта.
- Для проверки исходных целей нужны 100–300 реальных произнесений,
  разные говорящие, quiet/noisy RU/EN и не менее 10 часов реальных
  негативов. Отчёт явно показывает объём выбранного evaluation split;
  при количестве меньше 100/10ч он имеет `insufficient_data`.

```powershell
python tools/wake-bench/bench.py validate --corpus build/my-wake-corpus/corpus.json
```

## Рабочие точки и закрытая выборка

Встроены baseline (250 мс, 2 подтверждения, cooldown 2000 мс), responsive
(150/2/2000) и conservative (400/3/2000). Свой JSON через `--points`
принимает до девяти явно заданных точек с такими же полями и `id`.
Это дискретные настройки ASR matcher. Вероятностного score у этого
baseline нет: отчёт содержит FA/h–miss trade-off; полноценная DET
по порогу score остаётся будущему классификатору.

```powershell
$replay = Join-Path $env:CARGO_TARGET_DIR 'release/examples/wake_bench.exe'
python tools/wake-bench/bench.py run --corpus build/my-wake-corpus/corpus.json `
  --replay $replay --model-ru C:/models/wake-ru --model-en C:/models/wake-en `
  --mode realtime --split dev --output build/wake-dev.json
# Выберите одну точку по dev; test не используется для перебора.
python tools/wake-bench/bench.py run --corpus build/my-wake-corpus/corpus.json `
  --replay $replay --model-ru C:/models/wake-ru --model-en C:/models/wake-en `
  --mode realtime --split test --point baseline --selection build/wake-dev.json `
  --output build/wake-test.json
```

Test принимает одну точку из dev-отчёта при совпадении корпуса, бинарника,
моделей, runtime, runner, оборудования и режима replay. Изменение
параметров или lineage требует нового dev-прогона. Выбор test-точки
на основании результатов самого test нарушает методику, даже если
оператор повторяет CLI вручную.

## Метрики и ограничения

Recall считается отдельно для тишины и шума по реальным аннотациям.
Одна доставка сопоставляется одному произнесению; лишние события
остаются в отчёте. FA/h — события на реальных отрицательных WAV,
делённые на их часы. Синтетическая тишина не увеличивает этот знаменатель.
Отсутствующий знаменатель или измерение остаются `null`.

Каждые 20 мс подаются настоящие сэмплы файла, без синтетического хвоста.
В режиме `realtime` стенд ждёт sample-clock deadline и измеряет время
доставки события от старта replay. p50/p95 берутся от размеченного конца
фразы до доставки; ранняя доставка сохраняет отрицательное значение,
не зажимается в ноль. Окно сопоставления заканчивается через 2 с после
фразы. Более позднее событие не возвращает пропуску статус успеха.
EOF flush учитывается явно на том же sample clock.

Fast replay сохраняет sample-clock задержку, но не выдаёт её за
wall-clock latency или idle CPU. Загрузка модели отделена от replay.
CPU — сумма kernel/user time всех потоков процесса по
[GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes),
делённая на wall time replay и умноженная на 100. Это процент одного
ядра; число логических CPU в знаменателе не используется.
Измеряется ASR decoder, без стоимости устройства/AudioHub и приложения.

Исходные цели сохранены: quiet recall ≥95%, noisy ≥90%, FA/h ≤0,5,
p95 ≤800 мс, idle CPU <1% одного ядра. `qualified=true` возможен только
на test, без `--smoke`, с достаточными реальными данными и всеми
измеренными/пройденными критериями. Это машинный результат стенда;
решение о возвращении функции и Windows-приёмка остаются отдельными.

Отчёт сохраняет события, счётчики, hardware/model/binary/runtime hashes
и конфигурацию. Пути WAV/моделей, личности говорящих, тексты расшифровок
и сырой decoder stderr в него не включаются.

## Ограниченная проверка инструмента

```powershell
python tools/wake-bench/make_smoke.py --output-dir build/wake-smoke
python tools/wake-bench/bench.py run --corpus build/wake-smoke/corpus.json `
  --replay $replay --model-ru C:/models/wake-ru --model-en C:/models/wake-en `
  --mode realtime --split test --point baseline --smoke --output build/wake-smoke/report.json
```

Генератор использует два существующих публичных
[FLEURS WAV](../../src-tauri/tests/fixtures/stt-quality/manifest.json)
с ранее зафиксированной CC-BY-4.0/атрибуцией и два синтетических файла
тишины. Это инженерный smoke без позитивов. Его нулевые события
на коротких негативах не подтверждают FA/h на многочасовой записи,
recall или качество будущего детектора.
