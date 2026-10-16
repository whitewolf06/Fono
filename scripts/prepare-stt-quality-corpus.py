#!/usr/bin/env python3
"""Fetch a small, pinned public FLEURS subset without downloading whole archives.

Python standard library only. Downloads stop after ten test utterances per locale.
The checked-in manifest locks the resulting audio and source-byte hashes.
"""
import array
import hashlib
import json
import math
from pathlib import Path
import random
import struct
import tarfile
import urllib.request
import wave

REVISION = "437d9e667fa3a2a08a1957f810bf2c3b5050d221"
SOURCE = f"https://huggingface.co/datasets/google/fleurs/resolve/{REVISION}/data"
ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "src-tauri/tests/fixtures/stt-quality"


def fetch(url):
    return urllib.request.urlopen(url, timeout=60)


def decode(data):
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError("expected WAV")
    position, fmt, audio = 12, None, None
    while position + 8 <= len(data):
        name, size = struct.unpack_from("<4sI", data, position)
        start = position + 8
        if start + size > len(data):
            raise ValueError("truncated WAV")
        if name == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", data, start)
        elif name == b"data":
            audio = data[start:start + size]
        position = start + size + size % 2
    if fmt is None or audio is None or fmt[1] != 1 or fmt[2] != 16000:
        raise ValueError(f"unexpected source WAV format: {fmt}")
    if fmt[0] == 1 and fmt[5] == 16:
        return [value[0] for value in struct.iter_unpack("<h", audio)]
    if fmt[0] == 1 and fmt[5] == 32:
        return [max(-32768, min(32767, round(value[0] / 65536)))
                for value in struct.iter_unpack("<i", audio)]
    if fmt[0] == 3 and fmt[5] == 32:
        return [max(-32768, min(32767, round(value[0] * 32768)))
                for value in struct.iter_unpack("<f", audio)]
    raise ValueError(f"unsupported source WAV format: {fmt}")


def write_wav(path, samples):
    with wave.open(str(path), "wb") as output:
        output.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        pcm = array.array("h", samples)
        import sys
        if sys.byteorder != "little":
            pcm.byteswap()
        output.writeframes(pcm.tobytes())


def prepare():
    DESTINATION.mkdir(parents=True, exist_ok=True)
    entries = []
    for locale, language in [("en_us", "en"), ("ru_ru", "ru")]:
        metadata_url = f"{SOURCE}/{locale}/test.tsv"
        with fetch(metadata_url) as response:
            metadata = response.read(2_000_000).decode("utf-8")
        rows = {fields[1]: fields for line in metadata.splitlines()
                if len(fields := line.split("\t")) == 7}
        archive_url = f"{SOURCE}/{locale}/audio/test.tar.gz"
        count = 0
        with fetch(archive_url) as response:
            with tarfile.open(fileobj=response, mode="r|gz") as archive:
                for member in archive:
                    filename = Path(member.name).name
                    if not member.isfile() or filename not in rows:
                        continue
                    if member.size > 4_000_000:
                        raise ValueError("unexpectedly large utterance")
                    source_bytes = archive.extractfile(member).read()
                    samples = decode(source_bytes)
                    fields = rows[filename]
                    reference = fields[3]
                    condition = "clean"
                    if count == 3:
                        samples = [round(sample / 8) for sample in samples]
                        condition = "quiet-minus-18db"
                    elif count == 7:
                        rng = random.Random(20261017)
                        rms = math.sqrt(sum(sample * sample for sample in samples) / len(samples))
                        # Fixed white noise, 20 dB RMS SNR; no generated speech.
                        amplitude = rms / 10 * math.sqrt(3)
                        samples = [max(-32768, min(32767, round(sample + rng.uniform(-amplitude, amplitude))))
                                   for sample in samples]
                        condition = "white-noise-20db-snr"
                    elif count == 9:
                        samples = samples + [0] * 16000 + samples
                        reference = reference + " " + reference
                        condition = "long-repeat-with-1s-pause"
                    identifier = f"{language}-{count + 1:02d}"
                    path = DESTINATION / f"{identifier}.wav"
                    write_wav(path, samples)
                    entries.append({
                        "id": identifier, "language": language, "path": path.name,
                        "reference": reference, "condition": condition,
                        "samples": len(samples), "sample_rate": 16000,
                        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "source": {"dataset": "google/fleurs", "revision": REVISION,
                                   "locale": locale, "split": "test", "sentence_id": int(fields[0]),
                                   "filename": filename, "archive_member": member.name,
                                   "archive_url": archive_url, "metadata_url": metadata_url,
                                   "sha256": hashlib.sha256(source_bytes).hexdigest()},
                    })
                    count += 1
                    if count == 10:
                        break
        if count != 10:
            raise ValueError(f"only {count} utterances for {locale}")
        print(f"{locale}: {count} utterances", flush=True)
    manifest = {
        "schema": 1, "corpus": "fono-fleurs-smoke-v1", "license": "CC-BY-4.0",
        "attribution": "FLEURS, The Google and HuggingFace Datasets Authors (2022).",
        "source_card": f"https://huggingface.co/datasets/google/fleurs/blob/{REVISION}/README.md",
        "license_url": "https://creativecommons.org/licenses/by/4.0/legalcode.en",
        "selection": "First ten test archive WAV members per locale; chosen before recognition, never filtered by WER.",
        "limitations": "Small smoke corpus, derived 48 kHz replay is not native wideband capture or owner microphone acceptance.",
        "entries": entries,
    }
    (DESTINATION / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    prepare()
