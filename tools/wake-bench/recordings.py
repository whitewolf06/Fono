"""Audit local recording sets without recording, training, changing labels or publishing voices."""
from __future__ import annotations
import argparse
import collections
import hashlib
import json
from pathlib import Path
import sys
import uuid
import wave

PHRASES = {"privet_lira": {"ru": "Привет, Лира"},
           "hey_fono": {"ru": "Эй, Фоно", "en": "Hey Fono"}}


def read_json(path):
    if path.stat().st_size > 16384:
        raise ValueError("recording metadata is oversized")
    return json.loads(path.read_text(encoding="utf-8"))


def load_recordings(root):
    rows = []
    root = Path(root)
    if not root.is_dir():
        raise ValueError("recording directory is unavailable")
    for folder in sorted(root.iterdir()):
        if not folder.is_dir() or folder.name.startswith("."):
            continue
        try:
            identifier = str(uuid.UUID(folder.name))
        except ValueError:
            raise ValueError("invalid recording directory identity") from None
        if identifier != folder.name or folder.is_symlink() or folder.is_junction():
            raise ValueError("linked or invalid recording directory")
        for path in (folder / "recording.json", folder / "recording.wav"):
            if path.is_symlink() or path.is_junction() or not path.is_file():
                raise ValueError("recording files must be regular files")
        metadata = read_json(folder / "recording.json")
        sample = metadata["sample"]
        schema = metadata.get("schema")
        if schema not in ("fono-wake-recording@1", "fono-wake-recording@2"):
            raise ValueError("unknown recording schema")
        phrase_id = sample.get("phraseId", "hey_fono")
        language = sample["language"]
        if (metadata.get("permission") != "consented-local-only" or metadata.get("split") != "dev"
                or sample["id"] != identifier or phrase_id not in PHRASES
                or language not in PHRASES[phrase_id]
                or sample["kind"] not in ("phrase", "other_speech", "background")
                or sample["condition"] not in ("quiet", "noisy")):
            raise ValueError("recording provenance or declared group is invalid")
        if schema == "fono-wake-recording@1" and phrase_id != "hey_fono":
            raise ValueError("legacy recordings cannot belong to a new phrase")
        if schema == "fono-wake-recording@2" and ("phraseId" not in sample
                or metadata.get("phrase") != PHRASES[phrase_id][language]):
            raise ValueError("recording phrase identity mismatch")
        path = folder / "recording.wav"
        if not 44 < path.stat().st_size <= 22 * 16000 * 2 + 4096:
            raise ValueError("recording WAV is empty or oversized")
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != sample["sha256"]:
            raise ValueError("recording WAV hash mismatch")
        with wave.open(str(path), "rb") as audio:
            if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(), audio.getcomptype()) != (1, 2, 16000, "NONE"):
                raise ValueError("expected PCM16 mono 16 kHz")
            frames = audio.getnframes()
            pcm = audio.readframes(frames)
        if not frames or len(pcm) != frames * 2 or abs(frames // 16 - sample["durationMs"]) > 1:
            raise ValueError("truncated recording or duration mismatch")
        rows.append({"sample": sample, "phrase_id": phrase_id, "pcm": pcm,
                     "seconds": frames / 16000, "digital_silence": not any(pcm),
                     "annotation": metadata.get("annotation")})
    if len(rows) > 200:
        raise ValueError("recording collection exceeds the app limit")
    return rows


def audit(root, phrase_id):
    if phrase_id not in PHRASES:
        raise ValueError("unknown target phrase")
    rows = load_recordings(root)
    selected = [row for row in rows if row["phrase_id"] == phrase_id]
    groups = collections.defaultdict(list)
    for row in selected:
        s = row["sample"]
        groups[(s["language"], s["kind"], s["condition"])].append(row)
    report = {"schema": "fono-wake-recording-set-audit@1", "phrase_id": phrase_id,
              "phrases": PHRASES[phrase_id], "selected_recordings": len(selected),
              "other_set_recordings": len(rows) - len(selected),
              "selected_seconds": round(sum(r["seconds"] for r in selected), 3),
              "digital_silence_clips": sum(r["digital_silence"] for r in selected),
              "annotated_clips": sum(r["annotation"] is not None for r in selected),
              "groups": [{"language": k[0], "kind": k[1], "condition": k[2], "clips": len(v),
                          "seconds": round(sum(r["seconds"] for r in v), 3)} for k, v in sorted(groups.items())],
              "quality_measured": False, "qualified": False, "training_started": False,
              "status": "awaiting_recordings" if not selected else "recordings_present_unqualified"}
    # No filenames, audio hashes, opaque speaker/sample IDs, text or raw audio leave this report.
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True)
    parser.add_argument("--phrase-id", required=True, choices=sorted(PHRASES))
    args = parser.parse_args()
    try:
        print(json.dumps(audit(args.root, args.phrase_id), ensure_ascii=False, indent=2))
    except (ValueError, OSError, KeyError, wave.Error):
        print("Invalid local recording set; audit rejected.", file=sys.stderr)
        sys.exit(1)
