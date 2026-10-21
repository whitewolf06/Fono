import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import wave

import prepare


class PreparationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="fono-wake-training-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.corpus = self.root / "corpus.json"
        self.output = self.root / "prepared"
        self.manifest = {"schema": "fono-wake-corpus@1", "id": "fixture",
                         "phrases": prepare.bench.PHRASES, "entries": []}

    def clip(self, language="ru", split="train", positive=False, activity="speech", seconds=1):
        index = len(self.manifest["entries"])
        name = f"clip-{index}"
        path = self.root / f"{name}.wav"
        with wave.open(str(path), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            audio.writeframes(struct.pack("<h", index + 1) * int(seconds * 16000))
        item = {"id": name, "wav": path.name, "sha256": prepare.bench.sha256(path),
                "language": language, "split": split, "origin": "real", "condition": "quiet",
                "activity": activity, "speaker": name, "session": name, "source_group": name,
                "source": "private-local-evidence", "permission": "consented", "license": "fixture-only",
                "phrases": [{"start_ms": 200, "end_ms": 800}] if positive else []}
        self.manifest["entries"].append(item)
        return item

    def complete(self):
        for language in ("ru", "en"):
            for split in ("train", "dev"):
                self.clip(language, split, True)
                self.clip(language, split)

    def save(self):
        prepare.write_json(self.corpus, self.manifest)

    def test_test_is_never_used_for_training_or_upstream_validation(self):
        self.complete()
        excluded = self.clip(split="test", positive=True)
        self.save()
        result = prepare.prepare(self.corpus, self.output)
        self.assertEqual(result["excluded_test_clips"], 1)
        self.assertEqual(len(result["artifacts"]), 8)
        self.assertNotIn(excluded["id"], {x["source_id"] for x in result["artifacts"]})
        for artifact in result["artifacts"]:
            suffix = "_test/" if artifact["source_split"] == "dev" else "_train/"
            self.assertIn(suffix, artifact["file"])
        self.assertFalse(result["training_started"])
        self.assertFalse(result["qualified"])

    def test_negative_and_annotation_boundaries_keep_all_samples(self):
        self.complete()
        negative = self.clip(seconds=4.25)
        self.save()
        result = prepare.prepare(self.corpus, self.output)
        chunks = [x for x in result["artifacts"] if x["source_id"] == negative["id"]]
        self.assertEqual([(x["source_start_sample"], x["source_end_sample"]) for x in chunks],
                         [(0, 32000), (32000, 64000), (64000, 68000)])
        positive = result["artifacts"][0]
        self.assertEqual((positive["source_start_sample"], positive["source_end_sample"]), (3200, 12800))
        with wave.open(str(self.output / positive["file"]), "rb") as audio:
            self.assertEqual(audio.getnframes(), 9600)

    def test_oversize_positive_is_rejected_before_destination_creation(self):
        self.complete()
        item = self.clip(positive=True, seconds=3)
        item["phrases"] = [{"start_ms": 100, "end_ms": 2500}]
        self.save()
        with self.assertRaisesRegex(ValueError, "silently truncate"):
            prepare.prepare(self.corpus, self.output)
        self.assertFalse(self.output.exists())

    def test_split_leakage_and_invalid_permission_fail_closed(self):
        self.complete()
        self.manifest["entries"][2]["speaker"] = self.manifest["entries"][0]["speaker"]
        self.save()
        with self.assertRaisesRegex(ValueError, "split leakage"):
            prepare.prepare(self.corpus, self.output)
        self.manifest["entries"][2]["speaker"] = "different"
        self.manifest["entries"][0]["permission"] = "unknown"
        self.save()
        with self.assertRaisesRegex(ValueError, "permission"):
            prepare.prepare(self.corpus, self.output)
        self.assertFalse(self.output.exists())

    def test_smoke_negatives_and_synthetic_idle_cannot_supply_missing_positives(self):
        self.clip()
        self.clip(activity="idle")
        self.save()
        result, _ = prepare.inspect(self.corpus)
        self.assertFalse(result["data_ready"])
        self.assertIn("ru/train/positive", result["missing"])
        with self.assertRaisesRegex(ValueError, "missing training"):
            prepare.prepare(self.corpus, self.output)
        self.assertFalse(self.output.exists())

    def test_existing_destination_is_preserved(self):
        self.complete()
        self.save()
        self.output.mkdir()
        sentinel = self.output / "owner-data.txt"
        sentinel.write_text("preserve", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            prepare.prepare(self.corpus, self.output)
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
        self.assertEqual(list(self.output.iterdir()), [sentinel])

    def test_audio_change_after_validation_never_writes_complete_manifest(self):
        self.complete()
        self.save()
        inspect = prepare.inspect

        def changed(corpus):
            result = inspect(corpus)
            with (self.root / "clip-0.wav").open("ab") as stream:
                stream.write(b"changed")
            return result

        with patch.object(prepare, "inspect", side_effect=changed):
            with self.assertRaisesRegex(ValueError, "WAV changed"):
                prepare.prepare(self.corpus, self.output)
        self.assertFalse((self.output / "preparation.json").exists())

    def test_output_has_no_input_paths_or_source_text(self):
        self.complete()
        self.save()
        result = prepare.prepare(self.corpus, self.output)
        text = json.dumps(result)
        for forbidden in (str(self.root), "private-local-evidence", "fixture-only", "speaker", "_wav"):
            self.assertNotIn(forbidden, text)

    def test_preparation_is_byte_reproducible(self):
        self.complete()
        self.save()
        first = prepare.prepare(self.corpus, self.output)
        second = prepare.prepare(self.corpus, self.root / "second")
        self.assertEqual(first, second)
        for artifact in first["artifacts"]:
            self.assertEqual(prepare.bench.sha256(self.output / artifact["file"]), artifact["sha256"])

    def test_background_is_separate_and_config_never_requests_external_data(self):
        self.complete()
        idle = self.clip(activity="idle")
        self.save()
        result = prepare.prepare(self.corpus, self.output)
        artifact = next(a for a in result["artifacts"] if a["source_id"] == idle["id"])
        self.assertIn("background_train/", artifact["file"])
        for config in prepare.configs().values():
            self.assertEqual(config["n_samples"], 0)
            self.assertEqual(config["augmentation"]["background_paths"], [])
            self.assertEqual(config["augmentation"]["rir_paths"], [])
            self.assertNotIn("ACAV100M_sample", config["batch_n_per_class"])
            self.assertEqual(config["target_fp_per_hour"], 0.5)

    def test_cli_failure_is_nonzero_and_does_not_print_private_paths(self):
        error = io.StringIO()
        with patch("sys.argv", ["prepare.py", "inspect", "--corpus", str(self.root / "private-missing.json")]):
            with contextlib.redirect_stderr(error):
                self.assertEqual(prepare.main(), 1)
        self.assertNotIn(str(self.root), error.getvalue())


if __name__ == "__main__":
    unittest.main()
