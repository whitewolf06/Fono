"""Warm streaming contracts without recording audio or opening inference sessions."""
import io
import json
from pathlib import Path
import tempfile
import unittest
import numpy as np
import candidate as common
from live_worker import Diagnostics, Stream, classify_contexts, exact_read, validate_policy


class StreamTests(unittest.TestCase):
    def test_checked_in_policy_matches_live_runtime_before_microphone_open(self):
        validate_policy()

    def test_focused_context_uses_same_model_and_preserves_full_utterance(self):
        window = np.ones(common.WINDOW, dtype=np.float32)
        batches = []
        def predict(batch):
            batches.append(batch)
            return [[0.3], [0.9]]
        self.assertEqual(classify_contexts(window, predict), np.float64(0.9))
        self.assertTrue((window == 1).all())
        self.assertTrue((batches[0][0] == 1).all())
        self.assertTrue((batches[0][1, :common.WINDOW//2] == 0).all())
        self.assertTrue((batches[0][1, common.WINDOW//2:] == 1).all())
        for invalid in ([[0.9]], [[0.9], [float("nan")]], [[0.9], [1.1]]):
            with self.assertRaises(ValueError): classify_contexts(window, lambda _: invalid)

    def test_rejected_early_proposal_does_not_hide_completed_phrase(self):
        checks = []
        stream = Stream(lambda _: 0.9,
                        lambda _: checks.append(True) or len(checks) == 2)
        frame = np.ones(common.STRIDE, dtype=np.float32)
        detections = [stream.accept(frame, True)["detected"] for _ in range(30)]
        self.assertEqual(sum(detections), 1)
        self.assertEqual(len(checks), 2)

    def test_rejected_continuous_high_score_has_bounded_checks_and_no_activation(self):
        checks = []
        stream = Stream(lambda _: 0.9, lambda _: checks.append(True) or False)
        frame = np.ones(common.STRIDE, dtype=np.float32)
        for _ in range(150):
            self.assertFalse(stream.accept(frame, True)["detected"])
        self.assertEqual(len(checks), 3)
        self.assertEqual(stream.stats["retries"], 2)

    def test_pause_and_expired_retry_do_not_reuse_rejected_context(self):
        checks = []
        stream = Stream(lambda _: 0.9, lambda _: checks.append(True) or False)
        frame = np.ones(common.STRIDE, dtype=np.float32)
        for _ in range(9): stream.accept(frame, True)
        self.assertTrue(stream.retry)
        stream.reset()
        self.assertFalse(stream.retry)
        self.assertIsNone(stream.pending)
        for _ in range(9): stream.accept(frame, True)
        for _ in range(60): stream.accept(frame, False)
        self.assertFalse(stream.retry)
        self.assertIsNone(stream.pending)
        self.assertEqual(len(checks), 2)

    def test_proposal_waits_for_real_lookahead_and_high_score_does_not_retrigger(self):
        verified = []
        stream = Stream(lambda _: 0.9, lambda pcm: verified.append(pcm.copy()) or True)
        frame = np.ones(common.STRIDE, dtype=np.float32)
        self.assertFalse(stream.accept(frame, True)["detected"])
        for _ in range(common.TAIL // common.STRIDE - 1):
            self.assertFalse(stream.accept(frame, True)["detected"])
        self.assertTrue(stream.accept(frame, True)["detected"])
        self.assertEqual(len(verified[0]), common.STRIDE + common.TAIL)
        self.assertTrue((verified[0] == 1).all())
        for _ in range(90):
            self.assertFalse(stream.accept(frame, True)["detected"])
        self.assertEqual(len(verified), 1)
        self.assertLessEqual(len(stream.pcm), common.WINDOW + common.TAIL)

    def test_pause_reset_discards_pending_proposal_and_silence_skips_classifier(self):
        scores, verified = [], []
        stream = Stream(lambda pcm: scores.append(len(pcm)) or 0.9,
                        lambda pcm: verified.append(len(pcm)) or True)
        frame = np.zeros(common.STRIDE, dtype=np.float32)
        stream.accept(frame, True)
        stream.reset()
        for _ in range(20):
            self.assertFalse(stream.accept(frame, False)["detected"])
        self.assertEqual(scores, [common.WINDOW])
        self.assertEqual(verified, [])
        self.assertEqual(stream.processed, 20 * common.STRIDE)

    def test_invalid_frames_and_truncated_protocol_are_errors(self):
        stream = Stream(lambda _: 0.0, lambda _: False)
        for values in (np.zeros(1), np.full(common.STRIDE, float("nan"))):
            with self.assertRaises(ValueError): stream.accept(values, True)
        with self.assertRaises(EOFError): exact_read(io.BytesIO(b"short"), 8)
        self.assertEqual(exact_read(io.BytesIO(b"12345"), 5), b"12345")

    def test_repeated_wakes_after_dictation_reset_use_only_new_audio(self):
        snapshots = []
        stream = Stream(lambda _: 0.9, lambda pcm: snapshots.append(pcm.copy()) or True)
        for value in (0.1, 0.2, 0.3):
            stream.reset()
            frame = np.full(common.STRIDE, value, dtype=np.float32)
            hits = [stream.accept(frame, True)["detected"] for _ in range(30)]
            self.assertEqual(sum(hits), 1)
            self.assertTrue(np.allclose(snapshots[-1], value))
        self.assertEqual(stream.stats["accepted"], 3)

    def test_counters_survive_pause_without_storing_voice_or_text(self):
        stream = Stream(lambda _: 0.9, lambda _: True)
        for _ in range(9):
            stream.accept(np.ones(common.STRIDE, dtype=np.float32), True)
        stream.reset()
        stream.accept(np.zeros(common.STRIDE, dtype=np.float32), False)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "counters.json"
            diagnostics = Diagnostics(path)
            diagnostics.write(stream, force=True)
            value = json.loads(path.read_text())
            self.assertEqual(value["frames"], 10)
            self.assertEqual(value["speech_frames"], 9)
            self.assertEqual(value["proposals"], 1)
            self.assertEqual(value["checks"], 1)
            self.assertEqual(value["accepted"], 1)
            self.assertEqual(value["resets"], 1)
            self.assertEqual(value["processed"], common.STRIDE)
            self.assertEqual(set(value), {"schema", "pid", "updated_at", "processed", "frames",
                             "speech_frames", "max_abs", "max_cnn_score", "proposals", "checks", "accepted", "resets",
                             "retries", "max_classify_ms", "max_verify_ms", "last_check"})
            self.assertFalse(path.with_suffix(".tmp").exists())
            # Optional output failures never stop the live detector.
            Diagnostics(Path(directory) / "missing" / "counters.json").write(stream, force=True)


if __name__ == "__main__":
    unittest.main()
