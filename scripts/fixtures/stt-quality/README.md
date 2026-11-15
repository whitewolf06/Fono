# Числовые fixtures качества STT

Три JSON сохранены без изменения байтов для действующих потребителей:

- `stage2-stt-quality-final.json` и `stage4-quality.json` — входы unit-тестов
  `scripts/stage5-quality.test.mjs`. Тесты меняют копии в памяти и проверяют
  расчёт показателей, lineage и отрицательные критерии.
- `fono119-comparison.json` — закреплённый источник измерений для
  `src/v3/shared/domain/sttMeasurements.json`; exporter сохраняет путь и SHA-256.

Это исторические тестовые данные. Они не подтверждают качество, скорость,
оборудование или пользовательскую приёмку текущего релиза. Исходные версии,
пути и ограничения внутри JSON описывают момент соответствующего измерения.
Не исправляйте их задним числом при переименовании каталога.

Происхождение корпуса и границы replay описаны в
[README корпуса](../../../src-tauri/tests/fixtures/stt-quality/README.md).
Сохраняйте [исходную атрибуцию](../../../licenses/perf04-data-NOTICE.md),
[GPL v3 для соответствующих данных VoxForge](../../../licenses/perf04-voxforge-GPL-3.0.txt)
и [CC BY 4.0 для FLEURS](../../../src-tauri/tests/fixtures/stt-quality/LICENSE-CC-BY-4.0.txt).
Текущие контракты — в [STT](../../../docs/stt.md) и
[тестировании](../../../docs/testing.md).
