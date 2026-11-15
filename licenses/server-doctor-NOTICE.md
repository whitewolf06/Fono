# Fono Server embedded doctor speech fixtures

Doctor recognizes only these built-in public fixtures. It does not capture a
microphone, read a personal recording, persist a transcript or download audio.
Both resources contain signed little-endian 16-bit mono PCM at 16 kHz. WAV
headers were removed without changing the sample data.

## English

- Source: `samples/jfk.wav` in the official whisper.cpp repository, release
  v1.8.3, commit `2eeeba56e9edd762b4b38467bab96c2517163158`.
- [Pinned upstream file](https://github.com/ggml-org/whisper.cpp/blob/2eeeba56e9edd762b4b38467bab96c2517163158/samples/jfk.wav).
- Original WAV SHA-256: `59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e`.
- Embedded PCM SHA-256: `a29462b8ebd467318000e683b9117ade46230d3255ed2024e7db894abd9b38c9`.
- 176000 samples, 11 seconds.
- Upstream distribution uses the MIT license; its full text is retained in
  `apps/fono-server/resources/doctor-whisper-LICENSE.txt` and the server image.

## Russian

- Source: `test_wavs/example.wav` from the GigaAM v3 conversion repository,
  revision `a6039be7cee829a9044a69ac0ebaf1c191217c97`.
- [Pinned upstream file](https://huggingface.co/csukuangfj/sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16/blob/a6039be7cee829a9044a69ac0ebaf1c191217c97/test_wavs/example.wav).
- Original WAV SHA-256: `d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69`.
- Embedded PCM SHA-256: `539bf24c83e1b54fbb0e1d8102f5b76b1ea00dc197c3d2b64a2ea793a7e8c4a8`.
- 180640 samples, 11.29 seconds.
- The pinned repository's MIT notice identifies `Copyright (c) 2024 GigaChat
Team`. The full notice is retained in
  `apps/fono-server/resources/doctor-gigaam-LICENSE.txt` and the server image.

The probe checks a small set of expected word fragments and the actual selected
backend. This is an installation check; recognition quality needs its own
representative corpus and acceptance criteria.
