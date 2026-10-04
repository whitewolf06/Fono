"""Regenerate the synthetic audio fixtures; standard library only, no recordings."""

import pathlib
import struct
import wave

FIXTURE_ROOT = pathlib.Path(__file__).resolve().parent

# A 0.1-second, 400 Hz sawtooth generated with integer arithmetic.
samples = [(index % 40 - 20) * 400 for index in range(1600)]
with wave.open(str(FIXTURE_ROOT / "tone-16k-mono.wav"), "wb") as output:
    output.setnchannels(1)
    output.setsampwidth(2)
    output.setframerate(16000)
    output.writeframes(struct.pack("<1600h", *samples))

# MPEG-2 Layer III: 64 kb/s, 16 kHz, mono, no CRC or padding.
# Zero side information declares no spectral data: each 576-sample frame is silent.
header = bytes.fromhex("fff388c0")
frame = header + bytes(288 - len(header))
(FIXTURE_ROOT / "silence-16k-mono.mp3").write_bytes(frame * 8)
