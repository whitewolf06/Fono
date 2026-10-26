> Исторический отчёт конкретного этапа. Текущие возможности и открытые вопросы — в [STATUS](../../STATUS.md); навигация по отчётам — в [архиве](README.md).

# PERF-04: результаты проверки ресемплера

Дата: 18 октября 2026 года. Кандидат Fono v0.7.28 поверх v0.7.27
(`ad79364be63341a3011bff65b55f3610f4fe942c`). Задача —
[FONO-79 / PERF-04](https://kvorum.gorbach-dev.ru/tasks/FONO-79),
родитель — [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan).

**PERF-04 остаётся открытым, FONO-79 — в `in_progress`.** На свежем native
holdout выбранный Blackman дал 81 ошибку на 341 слово, обе базы — по 80.
Строгий нулевой WER-допуск не выполнен. Кандидат сохранён в ветке для review;
регрессия не объявляется устранённой.

На исходной linear-генерации финальный Blackman сокращает регрессию с
85→88 до **85→86 ошибок на 450 слов**, но нулевой допуск всё ещё не выполнен.
Все 20 его WAV побайтно совпали с первым опытом расширенного Blackman.
После финальных изменений harness оба replay выполнены заново: historical
exit 1, cardinal exit 0. Независимо пересчитаны 80 scored entries, проверены
80 output WAV SHA256 и все 51 окончательных source inputs.

Отдельный прогон с явно исправленной cardinal-реконструкцией 16→48 кГц
дал 85→85 ошибок. Все legacy WAV byte-exact совпадают с прежней базой.
Исходные WAV, тексты и нулевой допуск не менялись. Этот результат новой
методики не заменяет отрицательные исходную и финальную native проверки.

Дополнительный native diagnostic дал разные выводы для разных баз:
72→78 ошибок относительно линейной децимации и 79→78 относительно FIR,
который был в v0.7.27. Отрицательное сравнение сохранено. Эти числа не дают
основания объявлять качество на любом микрофоне подтверждённым.

## Что изменилось в аудио

В `fono-audio/audio_source/resample.rs` окно теперь вычисляет нужный момент
выходного сигнала напрямую. Прежняя реализация сначала фильтровала каждый
входной кадр, округляла его до PCM16, затем линейно интерполировала уже
округлённые значения. На дробных частотах это добавляло спад полосы.

Спецификация зафиксирована до первого распознавания кандидата:

| Параметр при выходе 16 кГц | Условие                                                                         |
| -------------------------- | ------------------------------------------------------------------------------- |
| Passband                   | 0–7 кГц, отклонение не более 0,1 дБ                                             |
| Stopband                   | От 8 кГц, подавление не менее 60 дБ                                             |
| Исходный критерий плана    | Синус 12 кГц при входе 48 кГц: подавление не менее 40 дБ                        |
| Cutoff                     | 7,5 кГц, окно Blackman                                                          |
| Фильтр при 48→16 кГц       | 289 коэффициентов, lookahead 3 мс                                               |
| Clock и границы            | Первый output соответствует t=0; zero extension и прежнее конечное число кадров |
| Память                     | History и банки фаз готовятся до обработки; горячий цикл без выделений          |

Для обычных rational clocks используются точные фазы: одна для 48/96/192→16
кГц и 160 для 44,1→16 кГц. Другие частоты используют ограниченную таблицу
и интерполяцию коэффициентов. Накопление остаётся в f64 до одного финального
округления и ограничения PCM16. FIR вычисляется только для выходных кадров.
Размер history и число банков ограничены и для нетипичных входных частот;
такие частоты не получают обещание той же полосы при ограниченной длине окна.

Симметричное нечётное окно и фильтрация перед децимацией соответствуют
[схеме polyphase resampling](https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.resample_poly.html).
У оконного sinc значение cutoff означает
[-6 дБ по амплитуде](https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.firwin.html),
поэтому прежние 7,2 кГц нельзя было считать границей плоской полосы.

## Почему replay теперь имеет два режима

Исходные FLEURS WAV уже имеют частоту 16 кГц. В прежнем replay линейная
интерполяция повышала её до 48 кГц. Такой треугольный фильтр ослабляет исходную
полосу и оставляет спектральные копии. Legacy при выборе каждого третьего
кадра возвращает исходные samples; корректный low-pass убирает копии, вместе
с которыми меняется результат обратного перехода. Этот опыт измеряет и
ресемплер, и искусственную реконструкцию. Он не равен записи микрофона 48 кГц.

Пятиаргументный example и запуск без нового флага сохраняют этот historical
linear режим. Результаты 85→88 и 85→86 не удалены и не объявляются пройденными.

Дополнительный явный `--bandlimited-reconstruction` строит одинаковый вход
для обоих ресемплеров через cardinal sinc: Nyquist исходника 8 кГц, окно
Blackman, половина поддержки 48 исходных кадров и unity каждой фазы.
На целой исходной позиции берётся сам PCM16 sample. Поэтому все 20 legacy
WAV сохраняют SHA256 из `baseline.json`. Обратного усиления для компенсации
треугольного фильтра и индивидуальных настроек по репликам нет.

Независимая NumPy-проверка cardinal construction сохранила все 20 hashes.
Converter дополнительно прошёл 40 проверок совместимости legacy: по 20
для прежнего и нового режимов. Native вход выбирается отдельным
`--native-input`; он проверяет частоту WAV и не повышает её из 16 кГц.
Эти два флага взаимоисключающие.

Положительный fixed-corpus результат относится к явно исправленному способу
реконструкции. Он не отменяет отрицательные результаты со старой генерацией
и сам по себе не доказывает качество на настоящем wideband входе.

## Результаты качества

Один pinned multilingual Whisper tiny, одинаковый CPU runner, fast decoding
и 8 потоков используются для сравниваемых вариантов. Модель:
`be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21`.
WER и CER пересчитаны независимо из reference/hypothesis; references,
полный PCM, decoding options и знаменатели не подбирались по результату.

| Опыт                                                 | WER: число ошибок | CER: число ошибок         | Результат                                        |
| ---------------------------------------------------- | ----------------- | ------------------------- | ------------------------------------------------ |
| v0.7.27, historical linear reconstruction            | 85→88 / 450       | Отдельный прежний receipt | CI-03 +1 п.п. проходил, строгий PERF-04 нет      |
| Финальный Blackman, historical linear reconstruction | 85→86 / 450       | 207→212 / 2846            | 20/20 WAV совпадают с первым опытом; gate FAILED |
| Выбранный Blackman, cardinal reconstruction          | 85→85 / 450       | 207→209 / 2846            | Нулевой WER-допуск выполнен, CER вырос на 2      |
| Native diagnostic, linear → Blackman                 | 72→78 / 255       | 116→129 / 1502            | Отрицательный результат сохранён                 |
| Тот же native diagnostic, deployed FIR → Blackman    | 79→78 / 255       | 128→129 / 1502            | WER улучшился на 1, CER вырос на 1               |
| Kaiser A66, cardinal reconstruction                  | 85→86 / 450       | Сохранено в raw report    | Кандидат отклонён по строгому fixed gate         |
| Свежий native holdout, linear → Blackman             | 80→81 / 341       | 146→142 / 2011            | Строгий WER gate не выполнен                     |
| Тот же свежий holdout, deployed FIR → Blackman       | 80→81 / 341       | 140→142 / 2011            | Строгий WER gate не выполнен                     |

Kaiser A66 был рассчитан отдельно по той же passband/stopband спецификации
до его STT-проверки: 195 taps, beta 6,31446, lookahead около 2,021 мс.
[Формула Kaiser](https://docs.scipy.org/doc/scipy/reference/generated/scipy.signal.kaiserord.html)
даёт приближённую оценку; коэффициенты проверены численно. Несмотря на
меньший расчётный расход и 78 ошибок на первом native diagnostic, fixed gate
этот вариант не прошёл. Он не включён в production. Другие параметры по WER
не перебирались.

Числа cold/model-load/warm и RTF описывают этот CPU-запуск. Внешняя нагрузка
хоста не контролировалась, ОС cache не очищался, принятого независимого
latency baseline нет. Норматив задержки не объявлен пройденным.

## Native WAV и независимая проверка реализации

Первый native набор содержит все десять записей `gav-20170207-ifk` и все
десять `icern300-20151115-dno`: 115,875 с, PCM16 mono 48 кГц. Он выбран
по метаданным и размеру до распознавания, затем использован как diagnostic.
Его отрицательный результат после обратной связи больше не считается
непросмотренной финальной приёмкой.

На всех 1 854 000 output samples выбранный Blackman byte-exact совпал с
независимой offline full convolution NumPy: симметричный Blackman289/fc7500,
zero padding, centered clock, каждый третий кадр, одно PCM16-округление.
Максимальная ошибка — 0 LSB, сдвиг — 0, начало и конец совпали. Legacy точно
равен `source48[::3]`. Признаков ошибки ring, дробной фазы, clock или границ
в этом опыте не найдено. Корректность DSP не превращает native WER-регрессию
в положительный результат.

Для дополнительного финального holdout до выбора production заморожены
20 других записей: все десять `vad-20160619-jeo` и все десять
`hchauhan-20160614-dzx`. Другие submitters, 206,625 с PCM16 mono 48 кГц.
Выбор основан только на capture metadata, доступности и общем размере
14 752 963 байта; сложность текста и recognition outcomes не использовались.
Manifest заморожен 18 октября в 11:59:43 UTC:
`8038bc25f66785f21c158884a17c1cda018da640b4deef710aacfda939936999`.
Blackman выбран 12:09:13 UTC, до inference этого набора.

Финальный fresh native replay завершён: Blackman — 81/341 ошибок слов,
обе базы — по 80/341. Рост равен одной ошибке, или 0,2933 п.п. CER Blackman
составил 142/2011 против 146 у linear и 140 у deployed FIR. Критерий WER
не выполнен; улучшение CER относительно одной базы не компенсирует его.
Параметры после этого результата не менялись.

Двадцать фраз двух голосов не позволяют установить общий эффект для всех
пользователей. Но и небольшой размер различия не даёт права объявить нулевой
допуск пройденным. Следующий шаг — заранее определить расширенный blind
native corpus и условия оценки, сохранив отдельную непросмотренную часть.
Искать следующее окно по ошибкам уже проверенных фраз нельзя. Новая методика
не должна переписывать эти отрицательные результаты или задним числом менять
их критерий приёмки.

Оба native набора имеют собственные GPL-3.0-or-later notices. Архивы и WAV
остаются build-only. Эталонные тексты и notices входят в JSON-отчёты вместе
с [атрибуцией и исходными notices](../../evidence/licenses/perf04-data-NOTICE.md) и
[полным GPLv3](../../evidence/licenses/perf04-voxforge-GPL-3.0.txt), отдельно
от CC-BY-4.0 FLEURS и MIT-кода Fono. Manifest хранит точные archive/member
URL, SHA256, заголовки и размер каждого файла. HTTPS старого repository
не прошёл TLS; публичные архивы получены по HTTP. Эти SHA256 закрепляют
локально полученные байты, а не удостоверяют upstream-подпись.

Каждый набор содержит два мужских голоса. WAV headers и catalog capture
metadata проверены; аналоговая полоса микрофонов не измерялась. Это небольшие
контрольные наборы, без утверждений о полном покрытии голосов и условий.

## Проверки и артефакты

Focused `fono-audio --no-default-features`: 23 unit tests и один allocation
test прошли. Проверены passband/stopband после PCM16, дробная фаза,
произвольные packet sizes, EOF и bounded allocation. Четыре example tests
проверили точную частоту native WAV, stereo/truncation rejection, cardinal
legacy identity и конечный clock. Strict all-targets Clippy прошёл.
Converter proof также прошёл 40 native conversions и шесть CLI reject cases.

UI — 273/273, release scripts — 48/48 в рабочем дереве, включая 20 отдельных
пользовательских cache tests. В состав коммита входят остальные 28 release
tests. Финальные quality contracts — 27/27; typecheck, lint, build,
dependency contracts 7/7, format и version check прошли.

Первый native-прогон сохранил отказ тестовой фикстуры: она отвечала
`shutdown-3` на произвольный третий запрос. Для этого теста добавлен worker
с построчным JSON-dispatch и точным входящим request ID; Ping проверяется
до намеренного освобождения. Второй прогон обнаружил прежнее ожидание
немедленного process exit после ошибки, хотя cleanup может быть передан
супервизору. Assert теперь ждёт подтверждения supervisor в существующем
тестовом EOF-дедлайне. Production-код, его таймауты и набор исключений не
менялись. Оба неуспешных прогона сохраняются в evidence.

Проверка CI теперь запускает `legacy_linear` и `cardinal_sinc` в независимых
matrix jobs с `fail-fast: false`, строгим нулевым допуском и отдельными
артефактами. Итоговый check с прежним именем `quality` требует успеха всей
матрицы; failed, skipped и cancelled не превращаются в success. Зелёный
cardinal больше не может скрыть failed historical gate. Поле `acceptance`
в отчёте явно ограничено конкретным `reconstruction_policy`.
Совместимое поле `comparison` по-прежнему хранит допуск CI +1 п.п.; при
`--require-no-wer-regression` решение принимает строгий `acceptance` и exit
code процесса. Положительный `comparison.passed` сам по себе не подтверждает
нулевой допуск или PERF-04.

**Полная native-матрица прошла 14/14 gates.** Default и CPU-only — по
571/571 тестов, включая по 478 тестов приложения; strict Clippy обоих
профилей, шесть feature modes и два release checks прошли. Все 307 native
исходников и конфигураций совпали с предварительным snapshot после прогона.
Тяжёлый opt-in lifecycle soak исключён явно, нового soak pass нет.

В dev/test-проверке отключены debug symbols и incremental-кэш, чтобы
соблюдать репозиторный лимит 20 ГиБ. Production-профили не менялись.
Release checks выполняли `cargo check`, без запуска приложения и installer.

Переносимые receipts:

- [Общий quality report со всеми опытами, спецификациями и oracle](evidence/perf04-quality.json).
- [Первый diagnostic, linear/current](evidence/perf04-wideband.json).
- [Тот же diagnostic, deployed/current](evidence/perf04-wideband-deployed.json).
- [Свежий финальный native holdout](evidence/perf04-wideband-final.json).
- [Полная native-матрица](evidence/perf04-native-final.json).
- [Проверки версии и проекта](evidence/perf04-validation.json).
- [Синхронизация задач Кворума](evidence/perf04-kvorum-sync.json).

До коммита подтверждены и прочитаны обратно девять операций Кворума:
восемь комментариев и возврат FONO-79 в `in_progress`. FONO-49 и FONO-47
остаются `in_progress`, CI-03 / FONO-108 — `in_review`, ручная FONO-112 —
`todo`. Отдельный комментарий в эпике с фактическим hash коммита создаётся
после сохранения этого receipt и не входит в эти девять операций.

Локальные raw материалы находятся в `build/checks/perf04/`,
`build/checks/perf04-fixed-cardinal/` и `build/perf04/`.
`selected-candidate.json` связывает выбор Blackman с source/binary/spec hashes;
`selected-blackman-spec.json` сохраняет независимые DSP-условия и причину
отклонения Kaiser. В `build/perf04/wideband*/` сохранены frozen manifests,
selection до STT и лицензии. Отдельные receipts сохраняют команды, хэши,
точные результаты и границы каждого прогона.

## Как повторить оба fixed replay

Команды выполняются из корня репозитория:

```powershell
./scripts/bootstrap-ci-resources.ps1
$env:CARGO_TARGET_DIR = "$PWD/src-tauri/target-codex-review"
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --no-default-features -p fono-audio --example resample-wav -p fono-stt-worker --bin fono-stt-quality
node scripts/stt-quality.mjs --target src-tauri/target-codex-review/release --download-model --require-no-wer-regression --bandlimited-reconstruction --report build/stt-quality/perf04-report.json
npm run cache:maintain
```

Для существующей закреплённой модели можно передать `--model PATH` вместо
`--download-model`. Автоматической загрузки без явного флага нет. Workflow
`stt-quality.yml` проверяет обе методики раздельно со strict flag;
cardinal flag добавляется только в соответствующий matrix job. Нулевая регрессия
проверяется относительно same-run legacy и frozen historical baseline;
CER сохраняется отдельно. Общие frontend/native команды — в [testing](../../testing.md).

Для исходного historical linear режима повторите команду без
`--bandlimited-reconstruction` и сохраните отдельный report:

```powershell
node scripts/stt-quality.mjs --target src-tauri/target-codex-review/release --download-model --require-no-wer-regression --report build/stt-quality/perf04-linear-report.json
```

Для финального кандидата здесь ожидается exit code 1: исходный результат
85→86/450 всё ещё нарушает строгий gate. Положительный cardinal report из
первого вызова хранится отдельно и его не заменяет.

Микрофон, hotkey, поля, overlay, GPU, часовой hardware idle и installer
проверяются отдельно в [FONO-112](https://kvorum.gorbach-dev.ru/tasks/FONO-112).
Эта работа их не запускала и не подтверждала. Публикация, установка и
приёмка владельцем остаются самостоятельными действиями.
