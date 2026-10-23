"""Warm streaming contracts without recording audio or opening inference sessions."""
import io
import unittest
import numpy as np
import candidate as common
from live_worker import Stream, exact_read


class StreamTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
