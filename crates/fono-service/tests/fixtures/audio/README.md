# Audio ingest test fixtures

The two files here are byte-for-byte copies of synthetic fixtures in
`src-tauri/tests/fixtures/audio`. They contain no microphone recordings and
keep `fono-service` tests independent of the desktop crate's directory.

- `tone-16k-mono.wav`: PCM16 mono, 16 kHz, 1600 samples of a 400 Hz sawtooth.
- `silence-16k-mono.mp3`: eight silent MPEG-2 Layer III frames, mono, 16 kHz.

The source fixtures can be regenerated with
`python -I -B src-tauri/tests/fixtures/audio/generate.py`.
