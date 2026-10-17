# Третий этап improvement-plan

Срез от 17 октября 2026 года, ветка `codex/fono-stage3-20261017`, исходники
v0.7.23. Основание — [FONO-47](https://kvorum.gorbach-dev.ru/tasks/FONO-47/fono-improvement-plan).
Владелец согласовал точный vendoring WhiteUI 0.6.0 и существующий канал/ключ
Fono для отдельного CUDA-пакета. Лицензионный текст WhiteUI оставлен как есть
до отдельного обновления библиотеки. Полная приёмка этапа ещё не завершена.
После восстановления MCP результаты записаны в семь задач и четыре
родительские карточки Кворума, затем прочитаны обратно. [Подтверждение
синхронизации](evidence/stage3-kvorum-sync.json) отделено от приёмки этапа.

## Объём и зависимости

| Карточка                                                                 | Пункт  | Локальный результат                                                                                                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------ | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [FONO-72](https://kvorum.gorbach-dev.ru/tasks/FONO-72/oss-02-npm)        | OSS-02 | Точный оригинальный WhiteUI 0.6.0 вендорен; package/lock и registry переключены. Anonymous Linux npm ci и проверки проходят. Лицензионный notice отложен владельцем до новой версии библиотеки.                                                                                                                                |
| [FONO-73](https://kvorum.gorbach-dev.ru/tasks/FONO-73/oss-03-cpu-only)   | OSS-03 | CPU-only feature, launcher, Tauri override, изолированные данные и реальный speech VAD. Код и автоматические проверки готовы к ревью; чистая Windows и голос требуют владельца.                                                                                                                                                |
| [FONO-74](https://kvorum.gorbach-dev.ru/tasks/FONO-74/oss-04-readme)     | OSS-04 | EN/RU README, скриншоты и GIF, CONTRIBUTING, SECURITY и issue forms подготовлены. Чистая установка зависимостей проверена без приватного реестра.                                                                                                                                                                              |
| [FONO-75](https://kvorum.gorbach-dev.ru/tasks/FONO-75/oss-05)            | OSS-05 | [Загрузчик и отдельный пакет](cuda-package-proposal.md) реализованы: подпись, версия, хэши, отмена, antirollback, защищённые слоты и idle activation. Проверен реальный unsigned-архив 409 МБ. Unsigned base NSIS — 19.767 MB, предел <100 MB выполнен. Подпись/публикация официального asset и GPU-приёмка остаются отдельно. |
| [FONO-107](https://kvorum.gorbach-dev.ru/tasks/FONO-107/ci-02-ui-oss-02) | CI-02  | Frontend workflow допускает fork PR без credentials: fork guard и WhiteUI secrets удалены. Linux anonymous source bootstrap проходит. Удалённый GitHub Actions/fork PR не запускался.                                                                                                                                          |
| [FONO-110](https://kvorum.gorbach-dev.ru/tasks/FONO-110/doc-01)          | DOC-01 | Разделены руководство, архитектура, QA и STATUS; старые адреса QA сохранены как переходы.                                                                                                                                                                                                                                      |
| [FONO-111](https://kvorum.gorbach-dev.ru/tasks/FONO-111/doc-02)          | DOC-02 | Четыре ADR: GPU workers, временный C++ Vulkan, заморозка WakeWord и SQLite. ARC-08 не объявлен закрытым.                                                                                                                                                                                                                       |

DOC-01/02 не имеют блокирующей реализации и выполнены первыми в локальном
коммите `7e4ad06` / v0.7.21. OSS-03 опирается на `fono-audio` первого этапа.
OSS-04 и CI-02 сохраняют зависимость от OSS-02; OSS-05 — от OSS-03 и ARC-01.
Открытая приёмка PERF-04 второго этапа не скрыта и не переопределена.

## CPU-профиль

`npm run dev:cpu` запускает Windows desktop с `cpu-only`, который включает
настоящий `speech-vad`. CUDA/Vulkan исключены из Cargo-выбора, поиска workers
и Tauri resources. Случайно найденный рядом worker не становится кандидатом;
Auto использует embedded CPU. Явно выбранный недоступный GPU возвращает ошибку.

Launcher использует отдельный debug-профиль `build/dev-cpu-profile` и порт
17833 по умолчанию. Настоящие credentials и автозапуск заблокированы правилами
существующей debug-изоляции; микрофон, hotkey и IPC остаются настоящими.
Окружение восстанавливается после выхода дочерней команды. CUDA/Vulkan SDK
и submodule Vulkan worker для этого пути не нужны.

LLVM/libclang остаётся обязательным. Проверка готовых upstream bindings
`whisper-rs-sys` выявила несовместимый Linux C ABI на Windows; этот вариант
убран без изменения crate или подавления assertions. Финальная сборка
генерирует Windows bindings. Остальные prerequisites указаны в
[development](development.md); на машине теста GPU SDK не удалялись, их
features и переменные отключались явно. Чистая машина требует отдельного прогона.

## Sherpa bootstrap

Повторный старт больше не создаёт успешные GUID-распаковки. Используется один
кэш на полный pinned SHA256. Каждый запуск проверяет архив и все реальные
файлы кэша по доверенному содержимому; marker не заменяет проверку байтов.
Первичная распаковка проходит в своём staging, проверяется целиком и атомарно
публикуется. Прерванный staging не блокирует новый запуск.

Hash и tar работают с одним ограниченным снимком архива. Проверяются размеры,
пути, exact tree, ссылки/reparse и компоненты путей записи от checkout до
archive/lock/resource/DLL. Изменённый published cache блокирует запуск и не
чинится поверх потенциально используемых DLL. Старые пользовательские GUID
каталоги и build caches не удалялись.

## WhiteUI: точный vendoring и решение владельца

Исходный tarball настоящей WhiteUI 0.6.0: 134150 байт,
SHA256 `e0aabc07070a54eae56cfb810f120b3ebdcb12f34e1313a1eecc073c5dfaab65`.
Все 95 файлов совпадают с установленным пакетом. Тег `v0.6.0` указывает на
`2fa5232000ab20349e35703ffd8fe7f5a0188167`; generated dist в теге отсутствует,
его пересборка не заявляется. Объявлена MIT, но полного LICENSE/copyright в
исходном архиве и теге нет. Лицензия более позднего Gavia назад не переносилась.

После решения владельца оригинальный tarball включён в `vendor/whiteui`,
package/lock используют локальный `file:` source с прежним integrity.
`.npmrc` использует публичный npm; CI больше не запрашивает WhiteUI token и
не исключает fork PR. Signing secrets защищённого release workflow сохранены.
Файлы LICENSE/copyright в библиотеку не добавлялись по явному решению владельца.
Полный provenance — [рядом с пакетом](../vendor/whiteui/README.md).

[Финальный anonymous Linux прогон](evidence/stage3-anonymous-vendored-final.json):
Node 22.23.3, npm 10.9.9, Debian 12 x64. Из точного Git tree
`8c6beeb82e1fc6d97e0fa753718888bb8f48d3ed` прошли `npm ci`, typecheck, lint,
UI **247/247**, tracked release **27/27**, format/version/build/docs. Все
752 исходных файла и manifests сохранили SHA256. Позднейшие Rust/CI-правки
проверены отдельно. Контейнер получал только snapshot исходников, пустые
npm-конфиги и собственную Linux filesystem; host npm cache и credentials не
монтировались. Образ закреплён digest, собственный контейнер удалён. Это
локальное воспроизведение anonymous CI; фактический fork workflow в GitHub
не запускался. [Предыдущий срез](evidence/stage3-anonymous-vendored.json)
сохранён отдельно.

## Реализация отдельного CUDA-пакета

В базовой поставке остаются CPU, точный Vulkan EXE и настоящий speech VAD.
Старые CUDA DLL из raw build resources исключены. Producer создаёт архив
из десяти файлов, включая шесть notices; DLL и тексты сверяются с локальным
native inventory. [Unsigned-архив](evidence/stage3-cuda-package.json) v0.7.23
занимает **409 019 734 байта**, SHA256
`8c9419e57981bd06b1df6ae0e2dffa6b800fab3e2585edef0ee7b789bdf0a695`.
Архив полностью проверен без исполнения CUDA worker.

[Локальный unsigned base NSIS](evidence/stage3-base-installer-unsigned.json)
v0.7.23 занимает **19 767 220 байт** (19.767 MB, 18.851 MiB); строгий предел **<100 000 000 байт**
выполнен. Сборка использовала обычный release-профиль, настоящий Vulkan worker,
Sherpa и VAD. CUDA в сгенерированный NSIS script не попала. Старые release-файлы
и исходные resources сохранили хэши. Подпись, установка, запуск приложения и
публикация при этом не выполнялись.

Runtime проверяет подпись manifest встроенным ключом, точные версии Fono и
protocol 3, purpose/platform/revision, SHA256 архива и каждого файла. Отмена
привязана к operation ID; один installer удерживает activity lease до cleanup.
Pointer и per-version high-water записываются атомарно. Замена модели и
активация выполняются в idle configuration transaction с восстановлением
прежнего выбора при ошибке. Recovery получает актуальный проверенный slot.

Windows file handles и OWNER RIGHTS DACL защищают используемый payload от
изменения и добавления DLL. Cache с epoch fence позволяет читать пути без
повторного хэширования сотен мегабайт на UI-потоках. Код прошёл отдельное
ревью; найденные I/O, rollback, stale cache и DLL-addition дефекты устранены.
При первой установке guards удерживают также промежуточные каталоги staging,
UUID и payload, затем передают защиту через promotion к проверенному slot.
Подмена родителя junction блокируется до записи файлов. После освобождения
UUID для атомарного rename cleanup больше не обращается к старому имени;
ошибка rename может оставить ограниченный неактивный staging для разбора.

В [browser mock](evidence/stage3-cuda-browser.json) проверены явная загрузка,
отмена, повтор, блокировка при несохранённых настройках и три viewport.
Скриншот — [CUDA settings](assets/cuda-package-demo.jpg). Native-приёмку этот
макет не подтверждает. Официальных подписанных assets пока нет; runtime
показывает их отсутствие без запуска worker.

## Финальные проверки v0.7.23

- [Windows native evidence](evidence/stage3-native-final.json): default и CPU-only workspace `--all-targets --include-ignored` — default **527/527**, CPU-only **527/527**; app tests — **453** и **453** соответственно, без пропусков. Публичный WAV прошёл настоящий pinned Silero VAD. Последний прогон использует `--test-threads=4`; внутренние тесты гонок и assertions сохранены.
- Rust fmt, strict Clippy `--workspace --all-targets -- -D warnings` для обоих профилей, шесть feature modes и два release-check с настоящими frontend/resources — прошли.
- Typecheck, lint, UI **247/247**, release scripts **47/47** в рабочем дереве, frontend build, format/version и license inventory check — прошли. В чистом Git snapshot release suite — **27/27**; дополнительные 20 cache tests относятся к исходным пользовательским файлам и остаются вне коммита этапа.
- Windows launcher fixtures — **10/10**, без пропусков; CI contract — **7/7**. CUDA producer Python — **12 passed, 1 skip** из-за недоступного права создания symlink; настоящий Windows junction проверен. Signing wrapper и Rust base-resource policy — по **4/4**. Официальный signing key в этих тестах не использовался.
- [Anonymous Linux](evidence/stage3-anonymous-vendored-final.json) проверил exact-tree source: npm ci без credentials, UI 247/247, release 27/27, typecheck/lint/format/version/build/docs и неизменность исходников/manifests. CPU launcher на Linux — 1 passed и 9 ожидаемых Windows skip. Позднейшие Rust/CI изменения проверены отдельными native/contract прогонами.
- [CUDA browser mock](evidence/stage3-cuda-browser.json): настройки на **720×560, 900×720, 1440×900**, явная загрузка, отмена, повтор и блокировка при несохранённом выборе; горизонтального переполнения и console warn/error нет. [Предыдущие девять browser-сценариев](evidence/stage3-browser-checks.json) для главной, обработки и тренера сохранены как отдельный срез.
- License inventory: 248 npm, 710 Cargo registry packages, прежние 141 evidence gap. Прямое объявление уже зафиксированного minisign-verify не добавило registry-версию; lock-хэши обновлены.

Логи находятся локально в `build/checks/stage3b-*.log`. Диагностические попытки сохранены отдельно: гонка с пересборкой dist, нехватка Python в PATH, ограничения AppContainer и timeout процессного fixture при чрезмерной параллельности. После исправления окружения и двух замечаний Clippy финальные наборы выполнены заново. Критерии не ослаблялись. Пользовательские изменения и старые release-артефакты сохранены.

## Кворум и продолжение

После серии транспортных ошибок MCP восстановлен по сообщению владельца.
Свежий bootstrap подтвердил server-resolved ready scope проекта Fono.
В семь задач и четыре родительские карточки добавлены результаты, решения
владельца, evidence и оставшаяся приёмка. Все комментарии и статусы прочитаны
обратно; [receipt](evidence/stage3-kvorum-sync.json) содержит ID записей.

FONO-72/73/74/110/111 имеют `in_review`; FONO-75/107 и родители
FONO-47/48/54/55 — `in_progress`. Завершёнными задачи не объявлялись.
Локальная очередь `build/checks/stage3-kvorum-pending.json` сохраняется для
аудита синхронизации; она не заменяет серверное подтверждение.

Решения владельца по WhiteUI и каналу CUDA получены и применены в v0.7.23.
Остаются подпись и публикация официального CUDA asset, фактический fork PR
workflow и Windows/GPU-приёмка. Наличие
локального кода и unsigned-архива не выдаётся за эти результаты.

Микрофон, пользовательский Fono, hotkey, внешняя вставка, installer и публикация
автоматически не запускались. Ухудшение WER PERF-04 на **0,6667 п.п.** остаётся
открытым в [предыдущем отчёте](improvement-stage-2.md) и [STATUS](STATUS.md).
