# Лицензии, notices и атрибуция

Общий реестр компонентов — [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md),
условия собственного кода — [MIT](../LICENSE), название и графика — [BRAND.md](BRAND.md).
Сторонние компоненты сохраняют свои условия независимо от MIT Fono.

Эти файлы перенесены из `docs/evidence/licenses/` без изменения байтов:

- [Офлайн-модели STT](offline-stt-NOTICE.md).
- [Встроенные записи doctor](server-doctor-NOTICE.md).
- [Публичная запись container smoke](server-smoke-NOTICE.md).
- [Корпус PERF-04](perf04-data-NOTICE.md) и [полный GPL v3](perf04-voxforge-GPL-3.0.txt).
- [FLEURS для длинной диктовки](fono124-long-dictation-NOTICE.md).

`offline-stt-NOTICE.md` входит в модельные пакеты: runtime сравнивает его точные
байты при проверке пакета. Поэтому перенос не должен менять текст, переводы строк
или кодировку. Два исходных notice — offline STT и длинной диктовки — содержат
относительные ссылки от прежнего каталога. Их текст сохранён как нормативный
payload; для актуальной навигации используйте ссылки здесь. Удалённые отчёты
эти notices не превращают в подтверждение текущего качества.

Актуальные источники и условия данных:

- [Закреплённый корпус и его границы](../src-tauri/tests/fixtures/stt-quality/README.md),
  [manifest](../src-tauri/tests/fixtures/stt-quality/manifest.json) и
  [CC BY 4.0](../src-tauri/tests/fixtures/stt-quality/LICENSE-CC-BY-4.0.txt).
- [Числовые fixtures инструментов качества](../scripts/fixtures/stt-quality/README.md).
- [FLEURS: карточка закреплённой версии](https://huggingface.co/datasets/google/fleurs/blob/437d9e667fa3a2a08a1957f810bf2c3b5050d221/README.md).
- [Whisper.cpp: публичная английская запись](https://github.com/ggml-org/whisper.cpp/blob/2eeeba56e9edd762b4b38467bab96c2517163158/samples/jfk.wav).
- [GigaAM: публичная русская запись](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16/blob/a6039be7cee829a9044a69ac0ebaf1c191217c97/test_wavs/example.wav).

Условия VoxForge и copyright исходных авторов сохранены в notice PERF-04.
Аудио, эталонные тексты, результаты и программный код могут иметь разные лицензии.
