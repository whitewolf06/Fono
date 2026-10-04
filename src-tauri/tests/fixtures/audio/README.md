# Synthetic decoder fixtures

These files contain generated signals, without speech or microphone recordings.
They are tracked test inputs, independent of the ignored Whisper worker vendor checkout.

- `tone-16k-mono.wav`: PCM16, mono, 16 kHz, 1600 samples / 0.1 seconds; an integer 400 Hz sawtooth.
- `silence-16k-mono.mp3`: eight MPEG-2 Layer III silent frames, mono, 16 kHz, 64 kb/s; 4608 decoded samples / 0.288 seconds.

Regenerate both files using `python -I -B src-tauri/tests/fixtures/audio/generate.py`.
The standard library is sufficient; no encoder installation or external downloads are required.
