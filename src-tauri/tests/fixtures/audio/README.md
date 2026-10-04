# Audio test fixtures

These are tracked test inputs, independent of the ignored Whisper worker vendor
checkout. No fixture contains a user's microphone recording.

- `tone-16k-mono.wav`: PCM16, mono, 16 kHz, 1600 samples / 0.1 seconds; an integer 400 Hz sawtooth.
- `silence-16k-mono.mp3`: eight MPEG-2 Layer III silent frames, mono, 16 kHz, 64 kb/s; 4608 decoded samples / 0.288 seconds.
- `jfk-short-speech-16k-mono.wav`: a 0.410-second public speech excerpt, samples
  3840 through 10399 (0.240–0.650 seconds) from the JFK inaugural-address WAV in
  [whisper.cpp](https://github.com/ggml-org/whisper.cpp/blob/master/samples/jfk.wav).
  PCM16, mono, 16 kHz. Tests also attenuate it to verify quiet speech; it is not
  a speech-quality or WakeWord recall corpus.

Regenerate the two synthetic files using `python -I -B src-tauri/tests/fixtures/audio/generate.py`.
The standard library is sufficient; no encoder installation or external downloads are required.
