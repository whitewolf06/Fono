import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest
import uuid
import wave

import recordings


class RecordingSetTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="fono-phrase-sets-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)

    def clip(self, phrase_id="privet_lira", legacy=False, value=7):
        folder = self.root / str(uuid.uuid4())
        folder.mkdir()
        path = folder / "recording.wav"
        with wave.open(str(path), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            audio.writeframes(struct.pack("<h", value) * 16000)
        sample = {"id": folder.name, "phraseId": phrase_id, "language": "ru", "kind": "phrase",
                  "condition": "quiet", "durationMs": 1000, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        meta = {"schema": "fono-wake-recording@1" if legacy else "fono-wake-recording@2",
                "sample": sample, "split": "dev", "permission": "consented-local-only", "annotation": None}
        if legacy:
            sample.pop("phraseId")
        else:
            meta["phrase"] = recordings.PHRASES[phrase_id]["ru"]
        metadata = folder / "recording.json"
        metadata.write_text(json.dumps(meta, ensure_ascii=False), encoding="utf-8")
        return folder, meta

    def test_old_fono_samples_never_count_as_lira_or_get_rewritten(self):
        old, _ = self.clip("hey_fono", legacy=True)
        before = (old / "recording.json").read_bytes()
        report = recordings.audit(self.root, "privet_lira")
        self.assertEqual(report["selected_recordings"], 0)
        self.assertEqual(report["other_set_recordings"], 1)
        self.assertEqual(report["status"], "awaiting_recordings")
        self.assertFalse(report["quality_measured"])
        self.assertEqual((old / "recording.json").read_bytes(), before)
        self.clip(value=8)
        self.assertEqual(recordings.audit(self.root, "privet_lira")["selected_recordings"], 1)

    def test_v2_cannot_omit_or_contradict_phrase_identity(self):
        folder, meta = self.clip()
        meta["phrase"] = "Эй, Фоно"
        path = folder / "recording.json"
        path.write_text(json.dumps(meta), encoding="utf-8")
        with self.assertRaises(ValueError):
            recordings.audit(self.root, "privet_lira")
        meta["phrase"] = "Привет, Лира"
        meta["sample"].pop("phraseId")
        path.write_text(json.dumps(meta), encoding="utf-8")
        with self.assertRaises(ValueError):
            recordings.audit(self.root, "privet_lira")

    def test_consent_hash_and_development_split_fail_closed(self):
        folder, original = self.clip()
        path = folder / "recording.json"
        for field, value in (("permission", "unknown"), ("split", "train"), ("split", "test")):
            meta = {**original, field: value}
            path.write_text(json.dumps(meta), encoding="utf-8")
            with self.assertRaises(ValueError):
                recordings.audit(self.root, "privet_lira")
        path.write_text(json.dumps(original), encoding="utf-8")
        with (folder / "recording.wav").open("ab") as stream:
            stream.write(b"corrupt")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            recordings.audit(self.root, "privet_lira")

    def test_aggregate_does_not_leak_audio_identity_and_silence_is_not_quality(self):
        folder, meta = self.clip(value=0)
        report = recordings.audit(self.root, "privet_lira")
        serialized = json.dumps(report)
        for private in (folder.name, str(folder), meta["sample"]["sha256"]):
            self.assertNotIn(private, serialized)
        self.assertEqual(report["digital_silence_clips"], 1)
        self.assertFalse(report["qualified"])
        self.assertFalse(report["quality_measured"])


if __name__ == "__main__":
    unittest.main()
