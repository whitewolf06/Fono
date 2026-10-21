import argparse
import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import wave
import bench


class BenchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fono-wake-bench-")
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.manifest = {"schema": "fono-wake-corpus@1", "id": "fixture", "phrases": bench.PHRASES, "entries": []}
        self.path = self.root / "corpus.json"

    def item(self, identifier="clip", split="dev", seed=1):
        path = self.root / f"{identifier}.wav"
        with wave.open(str(path), "wb") as output:
            output.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            output.writeframes(struct.pack("<h", seed) * 16000)
        item = {"id": identifier, "wav": path.name, "sha256": bench.sha256(path), "language": "ru", "split": split,
                "origin": "real", "condition": "quiet", "activity": "speech", "phrases": [],
                "speaker": identifier, "session": identifier, "source_group": identifier,
                "permission": "consented", "license": "fixture-only", "source": "local-consent-fixture"}
        self.manifest["entries"].append(item)
        return item

    def save(self):
        self.path.write_text(json.dumps(self.manifest), encoding="utf-8")
        return bench.validate_corpus(self.path)

    def trace(self):
        return {"schema": 1, "sample_rate": 16000, "audio_samples": 16000, "audio_duration_ms": 1000,
                "model_load_ms": 500, "replay_wall_ms": 1100, "process_cpu_ms": 8, "realtime": True, "events": []}

    def test_valid_corpus_has_explicit_insufficient_coverage(self):
        self.item()
        _, entries = self.save()
        self.assertEqual(entries[0]["duration_ms"], 1000)
        summary = bench.coverage(entries)
        self.assertFalse(summary["eligible"])
        self.assertIn("100 real annotated phrase utterances", summary["missing"])

    def test_hash_mismatch_is_not_an_empty_success(self):
        item = self.item()
        item["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            self.save()

    def test_alias_or_duplicate_audio_is_rejected(self):
        self.item("first")
        self.item("alias", split="test")
        with self.assertRaisesRegex(ValueError, "duplicate audio"):
            self.save()

    def test_each_group_prevents_cross_split_leakage(self):
        for key in ("speaker", "session", "source_group"):
            with self.subTest(key=key):
                self.manifest["entries"] = []
                first = self.item("first", seed=1)
                second = self.item("second", split="test", seed=2)
                second[key] = first[key]
                with self.assertRaisesRegex(ValueError, f"split leakage by {key}"):
                    self.save()

    def test_permission_and_annotation_are_required(self):
        item = self.item()
        item["permission"] = "unknown"
        with self.assertRaisesRegex(ValueError, "permission"):
            self.save()
        item["permission"] = "consented"
        item["phrases"] = [{"start_ms": 300, "end_ms": 1100}]
        with self.assertRaisesRegex(ValueError, "annotation"):
            self.save()

    def test_truncated_wave_does_not_reduce_exposure(self):
        item = self.item()
        audio = self.root / item["wav"]
        audio.write_bytes(audio.read_bytes()[:-200])
        item["sha256"] = bench.sha256(audio)
        with self.assertRaisesRegex(ValueError, "truncated WAV"):
            self.save()

    def test_nonfinite_json_and_points_are_rejected(self):
        self.path.write_text('{"score": NaN}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "invalid number"):
            bench.load_json(self.path)
        point = dict(bench.POINTS[0], confirmations=1)
        with self.assertRaises(ValueError):
            bench.validate_points([point])

    def test_one_delivery_cannot_detect_two_utterances_and_early_latency_is_signed(self):
        item = {"phrases": [{"start_ms": 100, "end_ms": 300}, {"start_ms": 500, "end_ms": 700}]}
        trace = self.trace()
        trace["events"] = [{"audio_ms": 250, "delivered_ms": 260}]
        scored = bench.score(item, trace)
        self.assertEqual(scored["matched"], 1)
        self.assertEqual(scored["latency_ms"], [-40])
        self.assertEqual(scored["sample_clock_latency_ms"], [-50])

    def test_extra_or_late_events_are_false_activations(self):
        trace = self.trace()
        trace["events"] = [{"audio_ms": 400, "delivered_ms": 450}, {"audio_ms": 500, "delivered_ms": 550}, {"audio_ms": 2900, "delivered_ms": 2950}]
        scored = bench.score({"phrases": [{"start_ms": 100, "end_ms": 300}]}, trace)
        self.assertEqual(scored["matched"], 1)
        self.assertEqual(scored["false_events"], 2)
        self.assertEqual(scored["latency_ms"], [150])

    def test_missing_denominators_remain_unknown_and_cannot_pass(self):
        metrics = bench.aggregate([])
        self.assertIsNone(metrics["false_activations_per_hour"])
        self.assertTrue(all(v == "not_measured" for v in bench.criteria(metrics).values()))

    def test_recall_false_activations_percentiles_and_one_core_cpu(self):
        common = {"origin": "real", "condition": "quiet", "activity": "speech", "latency_ms": [], "false_events": 0}
        rows = [{**common, "positives": 4, "matched": 3, "latency_ms": [100, 200, 900], "duration_ms": 1000,
                 "trace": {"realtime": True, "process_cpu_ms": 300, "replay_wall_ms": 1000}},
                {**common, "condition": "noisy", "positives": 2, "matched": 2, "latency_ms": [50, 300], "duration_ms": 1000,
                 "trace": {"realtime": True, "process_cpu_ms": 100, "replay_wall_ms": 1000}},
                {**common, "activity": "idle", "positives": 0, "matched": 0, "duration_ms": 7200000, "false_events": 3,
                 "trace": {"realtime": True, "process_cpu_ms": 57600, "replay_wall_ms": 7200000}}]
        metrics = bench.aggregate(rows)
        self.assertEqual(metrics["recall"], {"quiet": .75, "noisy": 1})
        self.assertEqual(metrics["false_activations_per_hour"], 1.5)
        self.assertEqual(metrics["latency_p50_ms"], 200)
        self.assertEqual(metrics["latency_p95_ms"], 900)
        self.assertAlmostEqual(metrics["cpu_percent_one_core"]["idle"], .8)
        self.assertEqual(metrics["cpu_percent_one_core"]["speech"], 20)
        gates = bench.criteria(metrics)
        self.assertEqual(gates["quiet_recall"], "failed")
        self.assertEqual(gates["false_activations_per_hour"], "failed")
        self.assertEqual(gates["latency_p95_ms"], "failed")

    def test_trace_must_match_audio_and_real_time_not_report_impossible_latency(self):
        self.item()
        item = self.save()[1][0]
        trace = self.trace()
        bench.validate_trace(trace, item, True)
        trace["audio_samples"] -= 1
        with self.assertRaisesRegex(ValueError, "truncated audio"):
            bench.validate_trace(trace, item, True)
        trace["audio_samples"] += 1
        trace["events"] = [{"audio_ms": 1000, "delivered_ms": 200}]
        with self.assertRaisesRegex(ValueError, "precedes available"):
            bench.validate_trace(trace, item, True)

    def test_test_point_is_frozen_to_dev_binary_models_and_corpus(self):
        point = bench.POINTS[0]
        lineage = {"corpus_sha256": "a", "binary_sha256": "b", "models": "c"}
        selection = {"run": {"split": "dev", "smoke": False}, "lineage": lineage, "points": [{"point": point}]}
        bench.verify_selection(selection, lineage, point)
        with self.assertRaisesRegex(ValueError, "lineage"):
            bench.verify_selection(selection, {**lineage, "binary_sha256": "changed"}, point)
        with self.assertRaisesRegex(ValueError, "operating point"):
            bench.verify_selection(selection, lineage, bench.POINTS[1])
        selection["run"]["split"] = "test"
        with self.assertRaisesRegex(ValueError, "dev run"):
            bench.verify_selection(selection, lineage, point)

    def test_runner_never_qualifies_smoke_and_omits_paths_speaker_and_stderr(self):
        item = self.item(split="test")
        self.save()
        replay = self.root / "replay.exe"
        replay.write_bytes(b"test binary identity")
        for name in bench.MODEL_FILES["ru"]:
            (self.root / name).write_bytes(b"test model identity")
        args = argparse.Namespace(corpus=str(self.path), split="test", points=None, point="baseline", selection=None, smoke=True,
                                  model_ru=str(self.root), model_en="", replay=str(replay), mode="realtime", output=str(self.root / "report.json"))
        completed = subprocess_result(self.trace())
        with patch("bench.subprocess.run", return_value=completed) as execute, contextlib.redirect_stdout(io.StringIO()):
            report = bench.run(args)
        self.assertFalse(report["qualified"])
        self.assertEqual(report["quality_status"], "insufficient_data")
        serialized = json.dumps(report)
        self.assertNotIn(str(self.root), serialized)
        self.assertNotIn('"speaker"', serialized)
        self.assertNotIn("PRIVATE STDERR", serialized)
        self.assertFalse(execute.call_args.kwargs.get("shell", False))
        args.smoke = False
        with self.assertRaisesRegex(ValueError, "frozen dev point"):
            bench.run(args)


def subprocess_result(trace):
    return argparse.Namespace(returncode=0, stdout=json.dumps(trace), stderr="PRIVATE STDERR")


if __name__ == "__main__":
    unittest.main()
