# Fono Server container smoke fixture

The smoke downloads `samples/jfk.wav` from the official
[whisper.cpp repository](https://github.com/ggml-org/whisper.cpp/blob/2eeeba56e9edd762b4b38467bab96c2517163158/samples/jfk.wav).

- Upstream release: v1.8.3, commit `2eeeba56e9edd762b4b38467bab96c2517163158`.
- SHA-256: `59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e`.
- Content: a short excerpt from John F. Kennedy's inaugural address, January 20, 1961.
- Upstream distribution: the whisper.cpp repository uses the
  [MIT license](https://github.com/ggml-org/whisper.cpp/blob/2eeeba56e9edd762b4b38467bab96c2517163158/LICENSE).

No personal recordings are used. The fixture is downloaded into memory during
the smoke, is size-bounded and hash-checked, and is not included in the image.
